//! The registry: every plugin the composition root registers, in that
//! order, checked once when the backend starts, then each plugin a program
//! declares while it runs, checked as it is declared. What the startup
//! catalog does not serve is left out: a command tree, a user stream or a
//! page method bound to an operation the catalog lacks.
//!
//! A registry never changes: a program's declaration or removal of one of
//! its plugins makes the next ([`Plugins`]). A plugin keeps its index in
//! every later registry, so a command set or a request that names a plugin
//! by its index names it whatever was declared since. A removed plugin
//! keeps its index, and no answer of the registry includes it.
//!
//! A program's plugin may be declared in versions beside its current
//! declaration. A version gives a conversation that names it its commands
//! and its context; everything else of the plugin, its page, tabs,
//! packages, streams and sign-in, is its current declaration's, which a
//! plugin declared only in versions lacks. Each version is checked alone as
//! it is declared, and with the other plugins as a conversation names it
//! ([`Registry::check_versions`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError, RwLock};

use jush_command_declarations::{LeafKind, NativeOperation, Node};
use jush_device_interface::RegisterError;
use jush_plugin_interface::{
    Commands, JUSH_ROOT, Manifest, Page, Placement, PluginFactory, PluginFile, PluginId, Scope, State, Stream,
};

use crate::commands::{TAKEN_GROUPS, compose};
use crate::features::{self, FeatureError};

/// Why the backend cannot start with its plugins; the error names the
/// plugin.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("two plugins have the id \"{0}\"")]
    DuplicateId(PluginId),
    #[error("plugin \"{plugin}\" declares \"{command}\", which is taken")]
    Taken { plugin: PluginId, command: String },
    #[error("plugin \"{plugin}\" declares the user stream \"{name}\", which is taken")]
    Stream { plugin: PluginId, name: String },
    #[error("plugin \"{plugin}\"'s page package \"{package}\" is another plugin's")]
    PagePackage { plugin: PluginId, package: String },
    #[error("plugin \"{plugin}\" declares the tab kind \"{kind}\", which is taken")]
    TabKind { plugin: PluginId, kind: String },
    #[error("plugin \"{plugin}\"'s commands are refused: {error}")]
    Commands {
        plugin: PluginId,
        error: RegisterError,
    },
    #[error("plugin \"{plugin}\"'s OAuth sign-in names {issuer}; an issuer is an https URL without a query")]
    OAuthIssuer { plugin: PluginId, issuer: String },
    #[error("plugin \"{plugin}\"'s OAuth sign-in names no vendor")]
    OAuthVendor { plugin: PluginId },
    #[error("plugin \"{plugin}\"'s features are refused: {error}")]
    Features { plugin: PluginId, error: FeatureError },
    #[error("plugin \"{0}\" is the release's own, which has no versions")]
    Unversioned(PluginId),
}

/// One registered plugin, and what of its manifest the catalog serves.
#[derive(Clone)]
pub(crate) struct Registered {
    pub(crate) factory: Arc<dyn PluginFactory>,
    pub(crate) commands: Vec<Commands>,
    pub(crate) streams: Vec<Stream>,
    pub(crate) page: Option<Page>,
    /// The packages its served commands, streams and methods bind: the
    /// only ones its package calls may name.
    pub(crate) packages: BTreeSet<String>,
    /// The number of its program's declaration of it, which a toolset's
    /// revision names; none for a plugin linked into the program.
    pub(crate) declaration: Option<u64>,
    /// Whether its current declaration stands: a program's plugin that its
    /// program removed, or declared only in versions, has none, and its
    /// factory is then a version's, which names its id.
    pub(crate) current: bool,
    /// The versions of it a program declared beside its current one, by
    /// name: each served as the current one is, for the conversations that
    /// name it, its own versions none.
    pub(crate) versions: BTreeMap<String, Registered>,
}

impl Registered {
    pub(crate) fn id(&self) -> &PluginId {
        &self.factory.manifest().id
    }

    /// The declaration a conversation that names `version` of it runs on,
    /// the current one for none; none when that one is not declared.
    pub(crate) fn chosen(&self, version: Option<&str>) -> Option<&Registered> {
        match version {
            Some(version) => self.versions.get(version),
            None => self.current.then_some(self),
        }
    }

    /// Whether the plugin is declared in some version, the current one or
    /// another.
    pub(crate) fn has_declaration(&self) -> bool {
        self.current || !self.versions.is_empty()
    }

    /// Its page's state of `scope`, if it declares one.
    pub(crate) fn state(&self, scope: Scope) -> Option<&State> {
        self.page.as_ref().and_then(|page| page.state(scope))
    }

    /// Whether it declares the tab kind `kind`.
    pub(crate) fn owns_kind(&self, kind: &str) -> bool {
        self.page
            .as_ref()
            .is_some_and(|page| page.tab_kinds.iter().any(|owned| owned == kind))
    }
}

/// The program's plugins as they are now: the registry it started with,
/// which each declaration and removal of one of its plugins replaces. A
/// core reads the newest, and adopts each next one
/// ([`crate::UserPlugins::adopt`]).
pub struct Plugins {
    current: RwLock<Arc<Registry>>,
    serves: Box<dyn Fn(&NativeOperation) -> bool + Send + Sync>,
}

impl Plugins {
    /// The registry of `factories` ([`Registry::new`]), whose later
    /// declarations `serves` checks as it checked these.
    pub fn new(
        factories: Vec<Box<dyn PluginFactory>>,
        serves: impl Fn(&NativeOperation) -> bool + Send + Sync + 'static,
    ) -> Result<Self, RegistryError> {
        let registry = Registry::new(factories, &serves)?;
        Ok(Self {
            current: RwLock::new(Arc::new(registry)),
            serves: Box::new(serves),
        })
    }

    /// The registry now.
    pub fn current(&self) -> Arc<Registry> {
        self.current.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Declares `factory`, a plugin of the program's, as its `version`, or
    /// as its current one for none, in place of the one declared so or after
    /// the others; the manifest declared already leaves the registry as it
    /// is. A manifest that breaks a rule, or the id of a plugin linked into
    /// the program, is refused.
    pub fn declare(&self, factory: Arc<dyn PluginFactory>, version: Option<&str>) -> Result<(), RegistryError> {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(next) = current.with_declared(factory, version, &self.serves)? {
            *current = Arc::new(next);
        }
        Ok(())
    }

    /// Removes the program's plugin `id` as its `version`, or its current
    /// declaration for none, the others staying: answers its id, or none
    /// when no program declared it so.
    pub fn remove(&self, id: &str, version: Option<&str>) -> Option<PluginId> {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        let next = current.without(id, version)?;
        let removed = current.declared_plugin(id)?.1.id().clone();
        *current = Arc::new(next);
        Some(removed)
    }
}

/// The program's plugins at one moment, shared by every shard thread.
pub struct Registry {
    /// Every plugin it ever registered, by its index; a removed one
    /// included.
    pub(crate) plugins: Vec<Registered>,
    /// The number of the last declaration.
    declarations: u64,
}

impl Registry {
    /// The plugins that are context sources in a version, the current one
    /// or another, in registration order, which a node asks before each
    /// provider request while its user has them on, as the version its
    /// conversation runs on is one.
    pub fn context_sources(&self) -> Vec<PluginId> {
        self.plugins
            .iter()
            .filter(|registered| {
                let context = |registered: &Registered| registered.factory.manifest().context;
                (registered.current && context(registered)) || registered.versions.values().any(context)
            })
            .map(|registered| registered.id().clone())
            .collect()
    }

    /// The registry of `factories`, in their order. What binds an operation
    /// `serves` refuses is left out, and logged; a manifest that breaks a
    /// rule stops the start.
    fn new(factories: Vec<Box<dyn PluginFactory>>, serves: &impl Fn(&NativeOperation) -> bool) -> Result<Self, RegistryError> {
        let mut plugins = Vec::new();
        for factory in factories {
            let factory: Arc<dyn PluginFactory> = Arc::from(factory);
            check_manifest(factory.manifest())?;
            plugins.push(served(factory, serves, None));
        }
        Self::checked(plugins, 0)
    }

    /// The id of `id`, a plugin a program declared, as its `version` or as
    /// its current one for none, while it is so.
    pub fn declared(&self, id: &str, version: Option<&str>) -> Option<PluginId> {
        self.declared_plugin(id)
            .filter(|(_, registered)| registered.declaration.is_some() && registered.chosen(version).is_some())
            .map(|(_, registered)| registered.id().clone())
    }

    /// This registry with the program's plugin `factory` as its `version`,
    /// or its current one for none, in place of the one declared so, or
    /// after the others; none when that manifest is declared so already.
    fn with_declared(
        &self,
        factory: Arc<dyn PluginFactory>,
        version: Option<&str>,
        serves: &impl Fn(&NativeOperation) -> bool,
    ) -> Result<Option<Self>, RegistryError> {
        let manifest = factory.manifest();
        check_manifest(manifest)?;
        let declaration = self.declarations + 1;
        let mut plugins = self.plugins.clone();
        let held = plugins.iter().position(|registered| registered.id() == &manifest.id);
        if let Some(index) = held
            && plugins[index].declaration.is_none()
        {
            return Err(RegistryError::DuplicateId(manifest.id.clone()));
        }
        match (held, version) {
            (Some(index), None) => {
                let held = &plugins[index];
                if held.current && held.factory.manifest() == manifest {
                    return Ok(None);
                }
                let versions = std::mem::take(&mut plugins[index].versions);
                plugins[index] = Registered { versions, ..served(factory, serves, Some(declaration)) };
            }
            (Some(index), Some(version)) => {
                let versions = &mut plugins[index].versions;
                if versions.get(version).is_some_and(|held| held.factory.manifest() == manifest) {
                    return Ok(None);
                }
                versions.insert(version.to_owned(), served(factory, serves, Some(declaration)));
            }
            (None, None) => plugins.push(served(factory, serves, Some(declaration))),
            // A plugin declared only as a version has no current one.
            (None, Some(version)) => {
                let declared = served(factory, serves, Some(declaration));
                let versions = BTreeMap::from([(version.to_owned(), declared.clone())]);
                plugins.push(Registered { current: false, versions, ..declared });
            }
        }
        Self::checked(plugins, declaration).map(Some)
    }

    /// This registry without the program's plugin `id` as its `version`,
    /// or its current one for none; none when no program declared it so.
    fn without(&self, id: &str, version: Option<&str>) -> Option<Self> {
        let (index, registered) = self.declared_plugin(id)?;
        registered.declaration?;
        registered.chosen(version)?;
        let mut plugins = self.plugins.clone();
        match version {
            Some(version) => {
                plugins[index].versions.remove(version);
            }
            None => plugins[index].current = false,
        }
        Some(Self {
            plugins,
            declarations: self.declarations,
        })
    }

    /// The registry of `plugins`, which no two of claim the same name of.
    fn checked(plugins: Vec<Registered>, declarations: u64) -> Result<Self, RegistryError> {
        let registry = Self { plugins, declarations };
        let mut ids = BTreeSet::new();
        let mut stream_names = BTreeSet::new();
        let mut page_packages = BTreeSet::new();
        let mut tab_kinds = BTreeSet::new();
        for registered in &registry.plugins {
            let manifest = registered.factory.manifest();
            // A removed plugin keeps its id, which a declaration takes again.
            if !ids.insert(manifest.id.clone()) {
                return Err(RegistryError::DuplicateId(manifest.id.clone()));
            }
            if !registered.current {
                continue;
            }
            for stream in &manifest.streams {
                if !stream_names.insert(stream.name.clone()) {
                    return Err(RegistryError::Stream {
                        plugin: manifest.id.clone(),
                        name: stream.name.clone(),
                    });
                }
            }
            if let Some(page) = &manifest.page {
                if !page_packages.insert(page.package.clone()) {
                    return Err(RegistryError::PagePackage {
                        plugin: manifest.id.clone(),
                        package: page.package.clone(),
                    });
                }
                for kind in &page.tab_kinds {
                    if !tab_kinds.insert(kind.clone()) {
                        return Err(RegistryError::TabKind {
                            plugin: manifest.id.clone(),
                            kind: kind.clone(),
                        });
                    }
                }
            }
        }
        registry.check()?;
        Ok(registry)
    }

    /// The plugins it has in their current declarations, by their indices.
    pub(crate) fn present(&self) -> impl Iterator<Item = (usize, &Registered)> {
        self.plugins
            .iter()
            .enumerate()
            .filter(|(_, registered)| registered.current)
    }

    /// Composes the command set once, with no instance behind it, so a name
    /// taken twice is found now and never while a conversation runs: of the
    /// current declarations together, and of each version alone, which a
    /// conversation that names it checks with the others
    /// ([`Self::check_versions`]). The `jush` root is the plugin host's, so
    /// a root of that name is taken whichever plugin comes first.
    fn check(&self) -> Result<(), RegistryError> {
        self.check_of(&BTreeMap::new(), |_| true)?;
        for (index, registered) in self.plugins.iter().enumerate() {
            for version in registered.versions.keys() {
                let versions = BTreeMap::from([(registered.id().to_string(), version.clone())]);
                self.check_of(&versions, |other| other == index)?;
            }
        }
        Ok(())
    }

    /// Whether a conversation may name `versions`, versions by plugin id: a
    /// plugin of the program's alone has versions, and the declarations it
    /// would run on take no name twice. A version not declared now names
    /// none of its plugin's commands.
    pub fn check_versions(&self, versions: &BTreeMap<String, String>) -> Result<(), RegistryError> {
        for id in versions.keys() {
            if let Some(registered) = self.plugins.iter().find(|registered| registered.id().as_str() == id)
                && registered.declaration.is_none()
            {
                return Err(RegistryError::Unversioned(registered.id().clone()));
            }
        }
        self.check_of(versions, |_| true)
    }

    /// [`Self::check`] of the declarations a conversation that names
    /// `versions` runs on, of the plugins `include` takes by their indices.
    fn check_of(&self, versions: &BTreeMap<String, String>, include: impl Fn(usize) -> bool) -> Result<(), RegistryError> {
        let mut jush = BTreeSet::new();
        for (_, registered) in self.chosen(versions).filter(|(index, _)| include(*index)) {
            for commands in &registered.commands {
                let name = commands.tree.name();
                let (taken, command) = match commands.placement {
                    Placement::Jush => (
                        TAKEN_GROUPS.contains(&name) || !jush.insert(name.to_owned()),
                        format!("{JUSH_ROOT} {name}"),
                    ),
                    Placement::Root => (name == JUSH_ROOT, name.to_owned()),
                };
                if taken {
                    return Err(RegistryError::Taken {
                        plugin: registered.id().clone(),
                        command,
                    });
                }
            }
        }
        compose(self, None, Vec::new(), versions, include, |_, _| true).map(|_| ())
    }

    /// The declarations a conversation that names `versions`, versions by
    /// plugin id, runs on, by the plugins' indices: a plugin it names a
    /// version of that is not declared has none.
    pub(crate) fn chosen<'a>(&'a self, versions: &'a BTreeMap<String, String>) -> impl Iterator<Item = (usize, &'a Registered)> {
        self.plugins.iter().enumerate().filter_map(|(index, registered)| {
            registered
                .chosen(versions.get(registered.id().as_str()).map(String::as_str))
                .map(|chosen| (index, chosen))
        })
    }

    /// The plugin that declares the user stream `name`, by its index.
    pub(crate) fn stream_owner(&self, name: &str) -> Option<usize> {
        self.present()
            .find(|(_, registered)| registered.streams.iter().any(|stream| stream.name == name))
            .map(|(index, _)| index)
    }

    /// The plugin that declares the user stream `name`.
    pub fn stream_plugin(&self, name: &str) -> Option<&PluginId> {
        self.stream_owner(name).map(|index| self.plugins[index].id())
    }

    /// The plugin that declares the tab kind `kind`.
    pub fn kind_plugin(&self, kind: &str) -> Option<&PluginId> {
        self.kind_owner(kind).map(|index| self.plugins[index].id())
    }

    /// Every user stream the catalog serves, of every plugin.
    pub fn streams(&self) -> impl Iterator<Item = &Stream> {
        self.present().flat_map(|(_, registered)| &registered.streams)
    }

    /// The plugin that declares the tab kind `kind`, by its index.
    pub(crate) fn kind_owner(&self, kind: &str) -> Option<usize> {
        self.present()
            .find(|(_, registered)| registered.owns_kind(kind))
            .map(|(index, _)| index)
    }

    /// The file `name` the plugin `plugin`'s pages load
    /// ([`PluginFactory::file`]), the same for every user.
    pub fn file(&self, plugin: &str, name: &str) -> Option<PluginFile> {
        self.plugin(plugin).and_then(|(_, registered)| registered.factory.file(name))
    }

    pub(crate) fn plugin(&self, id: &str) -> Option<(usize, &Registered)> {
        self.present().find(|(_, registered)| registered.id().as_str() == id)
    }

    /// The plugin `id` while it is declared, in its current version or
    /// another.
    pub(crate) fn declared_plugin(&self, id: &str) -> Option<(usize, &Registered)> {
        self.plugins
            .iter()
            .enumerate()
            .find(|(_, registered)| registered.id().as_str() == id && registered.has_declaration())
    }
}

/// Refuses a manifest that breaks a rule of its own.
fn check_manifest(manifest: &Manifest) -> Result<(), RegistryError> {
    if let Some(oauth) = &manifest.oauth {
        // RFC 8414 § 2: an https URL with no query or fragment.
        let issuer = &oauth.issuer;
        if issuer.scheme() != "https" || issuer.query().is_some() || issuer.fragment().is_some() {
            return Err(RegistryError::OAuthIssuer {
                plugin: manifest.id.clone(),
                issuer: issuer.to_string(),
            });
        }
        if oauth.vendor.trim().is_empty() {
            return Err(RegistryError::OAuthVendor {
                plugin: manifest.id.clone(),
            });
        }
    }
    features::check(&manifest.commands, &manifest.features).map_err(|error| RegistryError::Features {
        plugin: manifest.id.clone(),
        error,
    })
}

/// `factory` with what of its manifest `serves` serves.
fn served(
    factory: Arc<dyn PluginFactory>,
    serves: &impl Fn(&NativeOperation) -> bool,
    declaration: Option<u64>,
) -> Registered {
    let manifest: &Manifest = factory.manifest();
    let left_out = |what: &str, name: &str| {
        tracing::info!(
            event = "plugin_part.left_out",
            plugin = %manifest.id,
            what,
            name,
            reason = "not_served",
            "a part whose package the catalog does not serve is left out"
        );
    };
    let mut packages = BTreeSet::new();
    let mut bound = |operations: &[&NativeOperation]| {
        packages.extend(operations.iter().map(|operation| operation.package.clone()));
    };
    let mut commands = Vec::new();
    for tree in &manifest.commands {
        let mut operations = native_operations(&tree.tree);
        operations.extend(&tree.calls);
        if operations.iter().all(|operation| serves(operation)) {
            bound(&operations);
            commands.push(tree.clone());
        } else {
            left_out("command", tree.tree.name());
        }
    }
    let mut streams = Vec::new();
    for stream in &manifest.streams {
        if serves(&stream.operation) {
            bound(&[&stream.operation]);
            streams.push(stream.clone());
        } else {
            left_out("user stream", &stream.name);
        }
    }
    let page = manifest.page.as_ref().map(|page| {
        let mut page = page.clone();
        for (scope, state) in [
            ("user", &mut page.user),
            ("conversation", &mut page.conversation),
        ] {
            let kept = state.as_ref().is_none_or(|state| {
                let operations: Vec<&NativeOperation> = state.operations.iter().collect();
                let kept = operations.iter().all(|operation| serves(operation));
                if kept {
                    bound(&operations);
                }
                kept
            });
            if !kept {
                left_out("page state", scope);
                *state = None;
            }
        }
        page.methods.retain(|method| {
            let operations: Vec<&NativeOperation> = method.operations.iter().collect();
            let kept = operations.iter().all(|operation| serves(operation));
            if kept {
                bound(&operations);
            } else {
                left_out("page method", &method.name);
            }
            kept
        });
        page
    });
    Registered {
        commands,
        streams,
        page,
        packages,
        factory,
        declaration,
        current: true,
        versions: BTreeMap::new(),
    }
}

/// Every native operation a tree's leaves bind.
fn native_operations(tree: &Node<NativeOperation>) -> Vec<&NativeOperation> {
    match tree {
        Node::Leaf(leaf) => match &leaf.kind {
            LeafKind::Native(operation) => vec![operation],
            LeafKind::Rpc => Vec::new(),
        },
        Node::Group(group) => group
            .subcommands
            .iter()
            .flat_map(native_operations)
            .collect(),
    }
}

//! A program's page of one conversation, as the core answers it: a caller
//! bound to the conversation, with the operations its program gave it.
//!
//! The program's server hands the page's requests to the SDK, which sends
//! them on the process's channel below `/cores/{core}/pages/{conversation}/`
//! (`process/program.rs`). The page reaches:
//!
//! - the routes of [`ROUTES`], for its conversation alone, one that needs an
//!   operation only with it; any other route is not found ([`gate`]);
//! - the lanes [`lane`] lets it open, and of its conversation's lane the
//!   frames of its operations ([`frame_operation`]);
//! - the product state as [`state`] and [`event`] narrow it: its
//!   conversation, and of the rest what its views show;
//! - the blobs its conversation names or it uploaded ([`PageUploads`]), and
//!   the reply blocks its conversation names.
//!
//! What it reads of the conversation's settings leaves out the program's:
//! the system prompt, the commands and the browser profile.
//!
//! The operations are those its connection or request came with: a program
//! that gives a page others has it connect again.
//!
//! ```text
//! page -> program's server -> SDK -> /cores/c/pages/x/... -> gate -> route
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;

use axum::Json;
use axum::extract::{FromRequestParts, MatchedPath, RawPathParams, Request};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use jush_conversation_lane_protocol::{ClientFrame, ClientFrameKind, SavedDelivery};
use jush_core::Core;
use jush_core::conversation::LaneGate;
use jush_core::services::CoreServices;
use jush_shared_types::BlobRef;
use jush_web_api_protocol::connection::{CONNECT_ROUTE, LaneTarget};
use jush_web_api_protocol::conversations::{ConversationMetadata, ConversationPatch, ConversationSummary, ConversationTarget};
use jush_web_api_protocol::error::ErrorCode;
use jush_web_api_protocol::ids::{ConversationId, DeviceId, ProjectId, ProviderId, UserId, WorkspaceId};
use jush_web_api_protocol::page::{PageAnswer, PageOperation};
use jush_web_api_protocol::providers::{ModelCatalog, ProviderReading, ProviderState};
use jush_web_api_protocol::state::{CoreState, SyncEvent};
use jush_web_api_protocol::work::{Work, WorkChange};

use crate::caller::CallerOf;
use crate::error::ApiError;
use crate::state::StateError;

/// The header that names a page's operations, separated by commas, on the
/// process's channel.
pub const OPERATIONS_HEADER: &str = "jush-page-operations";

/// A page's conversation and what its program let it do.
#[derive(Debug, Clone)]
pub struct PageScope {
    conversation: ConversationId,
    allow: BTreeSet<PageOperation>,
}

impl PageScope {
    pub fn new(conversation: ConversationId, allow: impl IntoIterator<Item = PageOperation>) -> Self {
        Self {
            conversation,
            allow: allow.into_iter().collect(),
        }
    }

    /// The operations `header` names, separated by commas; none for an
    /// empty one.
    pub fn operations(header: &str) -> Result<BTreeSet<PageOperation>, ApiError> {
        header
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| {
                serde_json::from_value(serde_json::Value::String(name.to_owned()))
                    .map_err(|_| ApiError::invalid_body(format!("{name:?} is no operation of a page")))
            })
            .collect()
    }

    pub fn conversation(&self) -> &ConversationId {
        &self.conversation
    }

    pub fn allows(&self, operation: PageOperation) -> bool {
        self.allow.contains(&operation)
    }

    /// Refuses what needs an operation the page was not given.
    pub fn needs(&self, operation: Option<PageOperation>) -> Result<(), ApiError> {
        match operation {
            Some(operation) if !self.allows(operation) => Err(ApiError::forbidden(format!(
                "The program gave this page no {} operation",
                name(operation)
            ))),
            _ => Ok(()),
        }
    }

    /// Refuses another conversation than the page's, as one there is not.
    pub fn reaches(&self, conversation: &str) -> Result<(), ApiError> {
        if conversation == self.conversation.as_str() {
            Ok(())
        } else {
            Err(ApiError::new(
                StatusCode::NOT_FOUND,
                ErrorCode::ConversationNotFound,
                "Conversation not found",
            ))
        }
    }

    fn answer(&self) -> PageAnswer {
        PageAnswer {
            conversation: self.conversation.clone(),
            allow: self.allow.iter().copied().collect(),
        }
    }
}

/// An operation's name, as the program names it.
fn name(operation: PageOperation) -> String {
    serde_json::to_value(operation)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// What a page needs to reach a route.
#[derive(Debug, Clone, Copy)]
enum Needs {
    /// Nothing: it reads its conversation, or the route checks what it asks.
    Reading,
    Operation(PageOperation),
    /// The operation of the plugin its `{plugin}` names ([`plugin_operation`]).
    Plugin,
    /// A plugin's state, which needs the operation of its window if it is one
    /// ([`window_operation`]).
    PluginState,
}

use Needs::{Operation, Plugin, PluginState, Reading};
use PageOperation::{
    Ask, Attach, Changes, ChooseDevice, ChooseModel, Decide, Delete, Files, Fork,
    MessageAgents, Pins, Plugins, Schedules, Send, WriteFiles,
};

/// The routes a page reaches, by method and route, and what each needs; a
/// method of none is any, as the page connection's. A `HEAD` is its `GET`.
const ROUTES: &[(Option<Method>, &str, Needs)] = &[
    (Some(Method::GET), "/page", Reading),
    (Some(Method::GET), "/state", Reading),
    (None, CONNECT_ROUTE, Reading),
    (Some(Method::GET), "/conversations/{id}", Reading),
    // The fields each change needs (`patch`).
    (Some(Method::PATCH), "/conversations/{id}", Reading),
    (Some(Method::DELETE), "/conversations/{id}", Operation(Delete)),
    (Some(Method::POST), "/conversations/{id}/fork", Operation(Fork)),
    (Some(Method::POST), "/conversations/{id}/side-questions", Operation(Ask)),
    // A side question's fold, which its reader opens and closes.
    (Some(Method::PATCH), "/conversations/{id}/side-questions/{side}", Reading),
    (Some(Method::DELETE), "/conversations/{id}/side-questions/{side}", Operation(Ask)),
    (Some(Method::POST), "/conversations/{id}/side-questions/{side}/messages", Operation(Ask)),
    (Some(Method::POST), "/conversations/{id}/side-questions/{side}/retry", Operation(Ask)),
    (Some(Method::POST), "/conversations/{id}/side-questions/{side}/stop", Operation(Ask)),
    // The views read the conversation's lists and draft as they open it.
    (Some(Method::GET), "/conversations/{id}/workflows", Reading),
    (Some(Method::GET), "/conversations/{id}/agents/{node}/transcript", Reading),
    (Some(Method::POST), "/conversations/{id}/agents/{node}/messages", Operation(MessageAgents)),
    (Some(Method::GET), "/conversations/{id}/agents/{node}/blocks/{block}", Reading),
    (Some(Method::GET), "/conversations/{id}/turns/{turn}", Reading),
    (Some(Method::GET), "/conversations/{id}/commands/{command}", Reading),
    (Some(Method::GET), "/conversations/{id}/commands/{command}/output", Reading),
    (Some(Method::POST), "/conversations/{id}/read", Reading),
    (Some(Method::GET), "/conversations/{id}/attachments/{attachment}/file", Reading),
    (Some(Method::GET), "/conversations/{id}/cost/recent", Reading),
    (Some(Method::GET), "/conversations/{id}/search", Reading),
    // The windows each change needs (`work`).
    (Some(Method::POST), "/conversations/{id}/work/changes", Reading),
    (Some(Method::POST), "/conversations/{id}/suggested-replies/used", Operation(Send)),
    (Some(Method::GET), "/conversations/{id}/draft", Reading),
    (Some(Method::PUT), "/conversations/{id}/draft", Operation(Send)),
    (Some(Method::GET), "/conversations/{id}/saved", Reading),
    (Some(Method::POST), "/conversations/{id}/saved", Operation(Send)),
    (Some(Method::POST), "/conversations/{id}/saved/from-draft", Operation(Send)),
    (Some(Method::GET), "/conversations/{id}/saved/{message}", Reading),
    (Some(Method::PUT), "/conversations/{id}/saved/{message}", Operation(Send)),
    (Some(Method::DELETE), "/conversations/{id}/saved/{message}", Operation(Send)),
    (Some(Method::POST), "/conversations/{id}/permissions/requests/{request}", Operation(Decide)),
    (Some(Method::POST), "/conversations/{id}/plugins/{plugin}/calls/{method}", Plugin),
    (Some(Method::GET), "/conversations/{id}/plugins/{plugin}/state", PluginState),
    (Some(Method::POST), "/conversations/{id}/reload", Operation(Plugins)),
    (Some(Method::DELETE), "/conversations/{id}/pins/{pin}", Operation(Pins)),
    (Some(Method::POST), "/conversations/{id}/schedules", Operation(Schedules)),
    (Some(Method::GET), "/conversations/{id}/schedules/{schedule}", Operation(Schedules)),
    (Some(Method::PATCH), "/conversations/{id}/schedules/{schedule}", Operation(Schedules)),
    (Some(Method::DELETE), "/conversations/{id}/schedules/{schedule}", Operation(Schedules)),
    (Some(Method::GET), "/conversations/{id}/schedules/{schedule}/runs", Operation(Schedules)),
    (Some(Method::POST), "/conversations/{id}/schedules/{schedule}/runs", Operation(Schedules)),
    (Some(Method::GET), "/conversations/{id}/fs", Operation(Files)),
    (Some(Method::POST), "/conversations/{id}/fs", Operation(WriteFiles)),
    (Some(Method::DELETE), "/conversations/{id}/fs", Operation(WriteFiles)),
    (Some(Method::POST), "/conversations/{id}/fs/lists", Operation(Files)),
    (Some(Method::POST), "/conversations/{id}/fs/search", Operation(Files)),
    (Some(Method::POST), "/conversations/{id}/fs/html", Operation(Files)),
    (Some(Method::GET), "/conversations/{id}/fs/file", Operation(Files)),
    (Some(Method::GET), "/conversations/{id}/fs/raw", Operation(Files)),
    (Some(Method::PUT), "/conversations/{id}/fs/raw", Operation(WriteFiles)),
    (Some(Method::DELETE), "/conversations/{id}/devices/{device}", Operation(ChooseDevice)),
    (Some(Method::GET), "/conversations/{id}/devices/{device}/fs", Operation(Files)),
    (Some(Method::POST), "/conversations/{id}/devices/{device}/fs", Operation(WriteFiles)),
    (Some(Method::GET), "/conversations/{id}/changes", Operation(Changes)),
    (Some(Method::GET), "/conversations/{id}/changes/file", Operation(Changes)),
    (Some(Method::GET), "/conversations/{id}/changes/raw", Operation(Changes)),
    (Some(Method::GET), "/conversations/{id}/changes/branches", Operation(Changes)),
    (Some(Method::POST), "/conversations/{id}/changes/repositories", Operation(Changes)),
    (Some(Method::GET), "/conversations/{id}/changes/log", Operation(Changes)),
    (Some(Method::GET), "/conversations/{id}/changes/commit", Operation(Changes)),
    (Some(Method::POST), "/attachments", Operation(Attach)),
    // The blobs and reply blocks its conversation names (`blobs.rs`,
    // `blocks.rs`).
    (Some(Method::GET), "/blobs/{sha256}", Reading),
    (Some(Method::GET), "/plugins/{plugin}/blocks/{tag}/{id}", Reading),
    (Some(Method::POST), "/reply-blocks", Reading),
    (Some(Method::GET), "/plugins/{plugin}/files/{name}", Reading),
    (Some(Method::GET), "/models/search", Operation(ChooseModel)),
    (Some(Method::POST), "/models/refresh", Operation(ChooseModel)),
    // The folders a device offers to run in, and a new one.
    (Some(Method::GET), "/devices/{id}/fs", Operation(ChooseDevice)),
    (Some(Method::POST), "/devices/{id}/fs", Operation(ChooseDevice)),
    // An HTML file's frame carries no cookie, so its token is its only key:
    // the token was granted to a page with `files` (`fs/html`).
    (Some(Method::GET), "/html/{token}/", Reading),
    (Some(Method::GET), "/html/{token}/{*place}", Reading),
];

/// The operation a plugin's controls and its window need: the Browser
/// window's, the Simulators', the Changes', the Files', or `plugins` for
/// any other plugin. The windows are the plugins' pages' own
/// (`crates/plugin-*/page`).
pub fn plugin_operation(plugin: &str) -> PageOperation {
    match (window_operation(plugin), plugin) {
        (Some(operation), _) => operation,
        (None, "file-browser") => PageOperation::Files,
        (None, _) => PageOperation::Plugins,
    }
}

/// The operation of the window a plugin's page is, whose page state a page
/// reads only with it: the Browser's, the Simulators', the Changes'.
fn window_operation(plugin: &str) -> Option<PageOperation> {
    match plugin {
        "browser" => Some(PageOperation::Browser),
        "mobile" => Some(PageOperation::Simulators),
        "changes" => Some(PageOperation::Changes),
        _ => None,
    }
}

/// Lets a page's request reach its route when [`ROUTES`] has it, for the
/// page's conversation and with the operation it needs; any other
/// caller's request reaches its route as it is.
pub(crate) async fn gate(request: Request, next: Next) -> Response {
    let scope = request
        .extensions()
        .get::<CallerOf>()
        .and_then(|CallerOf(caller)| caller.scope().cloned());
    let Some(scope) = scope else {
        return next.run(request).await;
    };
    match admitted(&scope, request).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

async fn admitted(scope: &PageScope, request: Request) -> Result<Request, ApiError> {
    let (mut parts, body) = request.into_parts();
    let no_route = ApiError::no_route(&parts.method, parts.uri.path());
    let method = if parts.method == Method::HEAD { Method::GET } else { parts.method.clone() };
    let Some(route) = parts.extensions.get::<MatchedPath>().map(|matched| matched.as_str().to_owned()) else {
        return Err(no_route);
    };
    let Some((_, _, needs)) = ROUTES
        .iter()
        .find(|(of, template, _)| *template == route && of.as_ref().is_none_or(|of| *of == method))
    else {
        return Err(no_route);
    };
    let params = RawPathParams::from_request_parts(&mut parts, &()).await.map_err(|_| no_route)?;
    let param = |wanted: &str| params.iter().find(|(name, _)| *name == wanted).map(|(_, value)| value.to_owned());
    if route.starts_with("/conversations/{id}") {
        scope.reaches(&param("id").unwrap_or_default())?;
    }
    let operation = match needs {
        Reading => None,
        Operation(operation) => Some(*operation),
        Plugin => Some(plugin_operation(&param("plugin").unwrap_or_default())),
        PluginState => window_operation(&param("plugin").unwrap_or_default()),
    };
    scope.needs(operation)?;
    Ok(Request::from_parts(parts, body))
}

/// `GET /page`: the page's conversation and operations, which its views
/// read before they show anything.
pub(crate) async fn answer(CallerOf(caller): CallerOf) -> Result<Json<PageAnswer>, ApiError> {
    caller
        .scope()
        .map(|scope| Json(scope.answer()))
        .ok_or_else(|| ApiError::no_route(&Method::GET, "/page"))
}

/// Refuses a lane the page may not open: another conversation's, a
/// window's without its operation, and any lane but its synchronization
/// channel and its conversation's own.
pub(crate) fn lane(scope: &PageScope, target: &LaneTarget, services: &CoreServices) -> Result<(), ApiError> {
    match target {
        LaneTarget::Sync => Ok(()),
        LaneTarget::Conversation { conversation } => scope.reaches(conversation),
        LaneTarget::FileWatch { conversation } => {
            scope.reaches(conversation)?;
            scope.needs(Some(PageOperation::Files))
        }
        LaneTarget::UserStream { conversation, stream, .. } => {
            scope.reaches(conversation)?;
            let registry = services.plugins.current();
            let plugin = registry.stream_plugin(stream).map(|plugin| plugin.to_string()).unwrap_or_default();
            scope.needs(Some(plugin_operation(&plugin)))
        }
        // The folder dialog of the devices it may choose.
        LaneTarget::DeviceWatch { .. } => scope.needs(Some(PageOperation::ChooseDevice)),
        LaneTarget::Signaling { .. }
        | LaneTarget::Debug { .. }
        | LaneTarget::Starter
        | LaneTarget::Plugin { .. } => Err(ApiError::forbidden(
            "A program's page opens only its conversation's lanes",
        )),
    }
}

/// The operations a frame of the conversation lane needs; none for what
/// shows the conversation. A saved message steered is a saved message sent
/// and a steer, which takes it off the list; a close ends every turn of the
/// conversation as Stop does.
pub fn frame_operations(frame: &ClientFrame) -> &'static [PageOperation] {
    use ClientFrameKind as Kind;
    use PageOperation::{Clear, Compact, Edit, Retry, Steer, Stop};
    if let ClientFrame::SendSaved { delivery: SavedDelivery::Steer, .. } = frame {
        return &[Send, Steer];
    }
    match frame.kind() {
        Kind::Open | Kind::Follow | Kind::Unfollow => &[],
        Kind::Send | Kind::SendSaved | Kind::SendQueuedMessage | Kind::DequeueMessage | Kind::ClearMessageQueue => &[Send],
        Kind::Steer | Kind::SteerNow | Kind::CancelPendingSteer => &[Steer],
        Kind::EditAndSend | Kind::Regenerate | Kind::SwitchVersion => &[Edit],
        Kind::Abort | Kind::AbortSubagents | Kind::AbortSubagent | Kind::ShellAbort | Kind::Close => &[Stop],
        Kind::Retry | Kind::Resume => &[Retry],
        Kind::Compact => &[Compact],
        Kind::Clear => &[Clear],
    }
}

impl LaneGate for PageScope {
    fn admits(&self, frame: &ClientFrame) -> bool {
        frame_operations(frame).iter().all(|operation| self.allows(*operation))
    }

    fn shown(&self, metadata: &mut ConversationMetadata) {
        self.hide(metadata);
    }
}

impl PageScope {
    /// Leaves out of `metadata` what the page does not read: what the program
    /// set for the conversation, and the conversation state of a window's
    /// plugin the page may not open, as the Browser's tabs.
    pub fn hide(&self, metadata: &mut ConversationMetadata) {
        metadata.system_prompt = None;
        metadata.commands = None;
        metadata.browser_profile = None;
        metadata.tab_states.retain(|held| self.reads_conversation_state(&held.plugin));
    }

    /// Leaves out of `summary` the tabs of a window the page may not open,
    /// whose data says what they show, as a browser tab's address and title.
    pub fn hide_tabs(&self, summary: &mut ConversationSummary, services: &CoreServices) {
        let registry = services.plugins.current();
        summary.work.tabs.retain(|tab| {
            let plugin = registry.kind_plugin(&tab.kind).map(|plugin| plugin.to_string()).unwrap_or_default();
            self.reads_conversation_state(&plugin)
        });
    }

    /// Whether the page reads a plugin's state of its conversation: a
    /// window's with the window's operation, any other's as the
    /// conversation's.
    fn reads_conversation_state(&self, plugin: &str) -> bool {
        window_operation(plugin).is_none_or(|operation| self.allows(operation))
    }
}

/// Refuses a change of the conversation's settings the page may not make:
/// one of the program's or the sidebar's, or one of an operation it was not
/// given.
pub(crate) fn patch(scope: &PageScope, patch: &ConversationPatch) -> Result<(), ApiError> {
    let ConversationPatch {
        title,
        archived,
        pinned,
        model,
        thinking_effort,
        service_tier_id,
        workflow,
        browser_profile,
        system_prompt,
        commands,
        target,
    } = patch;
    if archived.is_some() || pinned.is_some() || browser_profile.is_some() || system_prompt.is_some() || commands.is_some() {
        return Err(ApiError::forbidden(
            "A program's page does not change the program's settings of its conversation, nor the sidebar's",
        ));
    }
    if title.is_some() {
        scope.needs(Some(PageOperation::Rename))?;
    }
    if model.is_some() || thinking_effort.is_some() || service_tier_id.is_some() {
        scope.needs(Some(PageOperation::ChooseModel))?;
    }
    if workflow.is_some() {
        scope.needs(Some(PageOperation::ChooseWorkflow))?;
    }
    if target.is_some() {
        scope.needs(Some(PageOperation::ChooseDevice))?;
    }
    Ok(())
}

/// The tab kinds of the Attachments window, which shows what the
/// conversation names: a page reads them with no operation.
const READ_KINDS: &[&str] = &["attachment", "media"];

/// Refuses a change of the work's tabs of a window the page may not open:
/// a tab is its kind's, a new one by its kind and any other by the tab it
/// names in `work`.
pub(crate) fn work(scope: &PageScope, work: &Work, changes: &[WorkChange], services: &CoreServices) -> Result<(), ApiError> {
    let registry = services.plugins.current();
    for change in changes {
        let kind = match change {
            WorkChange::Create(create) => Some(create.kind.as_str()),
            WorkChange::Update { id, .. } | WorkChange::Remove { id, .. } | WorkChange::Move { id, .. } => {
                work.tabs.iter().find(|tab| tab.id == *id).map(|tab| tab.kind.as_str())
            }
            WorkChange::Windows { .. } => None,
        };
        let Some(kind) = kind else { continue };
        if READ_KINDS.contains(&kind) {
            continue;
        }
        let plugin = registry.kind_plugin(kind).map(|plugin| plugin.to_string()).unwrap_or_default();
        scope.needs(Some(plugin_operation(&plugin)))?;
    }
    Ok(())
}

/// What a page's views show of the core beside its conversation: its
/// workspace and project, the devices it runs on and its model.
#[derive(Default)]
struct Beside {
    workspace: Option<WorkspaceId>,
    project: Option<ProjectId>,
    devices: BTreeSet<DeviceId>,
    provider: Option<ProviderId>,
    model: Option<String>,
}

impl Beside {
    async fn of(scope: &PageScope, core: &Core) -> Result<Self, StateError> {
        let Some(record) = core.store().conversation(scope.conversation.clone()).await? else {
            return Ok(Self::default());
        };
        let summary = core.conversation_summary(record.clone()).await?;
        let metadata = core.conversation_metadata(record).await?;
        let mut beside = Self {
            workspace: Some(summary.workspace_id.clone()),
            devices: metadata.devices.devices.iter().map(|attached| attached.device_id.clone()).collect(),
            provider: metadata.model.as_ref().map(|model| model.provider_id.clone()),
            model: metadata.model.as_ref().map(|model| model.model_id.clone()),
            ..Self::default()
        };
        match &summary.target {
            ConversationTarget::Device { device_id, .. } => {
                beside.devices.insert(device_id.clone());
            }
            ConversationTarget::Project { project_id } => {
                if let Some(project) = core.store().projects().await?.into_iter().find(|project| project.id == *project_id) {
                    beside.devices.insert(project.device);
                }
                beside.project = Some(project_id.clone());
            }
            ConversationTarget::Cloud { .. } => {}
        }
        Ok(beside)
    }
}

/// The product state as the page reads it: its conversation, and of the rest
/// what its views show.
pub async fn state(scope: &PageScope, core: &Core, state: CoreState) -> Result<CoreState, StateError> {
    let beside = Beside::of(scope, core).await?;
    let mut conversations: Vec<ConversationSummary> =
        state.conversations.into_iter().filter(|conversation| conversation.id == scope.conversation).collect();
    for conversation in &mut conversations {
        scope.hide_tabs(conversation, core.services());
    }
    Ok(CoreState {
        conversations,
        workspaces: state.workspaces.into_iter().filter(|workspace| beside.workspace.as_ref() == Some(&workspace.id)).collect(),
        projects: state.projects.into_iter().filter(|project| beside.project.as_ref() == Some(&project.id)).collect(),
        devices: if scope.allows(PageOperation::ChooseDevice) {
            state.devices
        } else {
            state.devices.into_iter().filter(|device| beside.devices.contains(&device.id)).collect()
        },
        providers: providers(scope, &beside, state.providers),
        models: models(scope, &beside, state.models),
        plugin_states: plugin_states(scope, state.plugin_states),
        logins: Vec::new(),
        stun_urls: Vec::new(),
        ..state
    })
}

/// `event` as the page's synchronization channel sends it, none when the
/// page sees nothing of it.
pub async fn event(scope: &PageScope, core: &Core, event: SyncEvent) -> Result<Option<SyncEvent>, StateError> {
    let needs_beside = matches!(
        event,
        SyncEvent::Workspaces { .. } | SyncEvent::Projects { .. } | SyncEvent::Devices { .. } | SyncEvent::Providers { .. } | SyncEvent::Models { .. }
    );
    let beside = if needs_beside { Beside::of(scope, core).await? } else { Beside::default() };
    Ok(match event {
        SyncEvent::Synced { .. } | SyncEvent::Stale { .. } | SyncEvent::Preferences { .. } | SyncEvent::Instance { .. } | SyncEvent::Plugins { .. } => Some(event),
        SyncEvent::Conversation { mut conversation } => (conversation.id == scope.conversation).then(|| {
            scope.hide_tabs(&mut conversation, core.services());
            SyncEvent::Conversation { conversation }
        }),
        SyncEvent::ConversationDeleted { id } => (id == scope.conversation).then_some(SyncEvent::ConversationDeleted { id }),
        SyncEvent::Workspaces { workspaces } => Some(SyncEvent::Workspaces {
            workspaces: workspaces.into_iter().filter(|workspace| beside.workspace.as_ref() == Some(&workspace.id)).collect(),
        }),
        SyncEvent::Projects { projects } => Some(SyncEvent::Projects {
            projects: projects.into_iter().filter(|project| beside.project.as_ref() == Some(&project.id)).collect(),
        }),
        SyncEvent::Devices { devices } => Some(SyncEvent::Devices {
            devices: if scope.allows(PageOperation::ChooseDevice) {
                devices
            } else {
                devices.into_iter().filter(|device| beside.devices.contains(&device.id)).collect()
            },
        }),
        SyncEvent::Plugin { plugin, state } => {
            reads_state(scope, &plugin).then_some(SyncEvent::Plugin { plugin, state })
        }
        SyncEvent::Providers { providers: entries } => Some(SyncEvent::Providers {
            providers: providers(scope, &beside, entries),
        }),
        SyncEvent::Models { models: catalog } => Some(SyncEvent::Models {
            models: models(scope, &beside, catalog),
        }),
        SyncEvent::ConversationOrder { .. }
        | SyncEvent::User { .. }
        | SyncEvent::Cloud { .. }
        | SyncEvent::Logins { .. }
        | SyncEvent::Subagents { .. }
        | SyncEvent::Instructions { .. }
        | SyncEvent::Credentials { .. }
        | SyncEvent::PortForwards { .. }
        | SyncEvent::Passkeys { .. }
        | SyncEvent::Sessions { .. }
        | SyncEvent::Users { .. }
        | SyncEvent::CloudIsolation { .. }
        | SyncEvent::ServerUpgrade { .. }
        | SyncEvent::Update { .. } => None,
    })
}

/// The entries the page's model menu shows: every one with `chooseModel`,
/// the conversation's own without; none with an address, accounts, quota or
/// an install of its tool.
fn providers(scope: &PageScope, beside: &Beside, entries: Vec<ProviderState>) -> Vec<ProviderState> {
    let every = scope.allows(PageOperation::ChooseModel);
    entries
        .into_iter()
        .filter(|entry| every || beside.provider.as_ref() == Some(&entry.provider.id))
        .map(|mut entry| {
            entry.provider.base_url = None;
            entry.provider.through_device = None;
            if let ProviderReading::Read(details) = &mut entry.details {
                details.accounts.clear();
                details.active = None;
                details.quota = None;
            }
            entry.cli_install = None;
            entry
        })
        .collect()
}

/// The models the page's model menu shows: every one with `chooseModel`,
/// the conversation's own without.
fn models(scope: &PageScope, beside: &Beside, mut catalog: ModelCatalog) -> ModelCatalog {
    if scope.allows(PageOperation::ChooseModel) {
        return catalog;
    }
    catalog.providers.retain(|provider| beside.provider.as_ref() == Some(&provider.provider_id));
    for provider in &mut catalog.providers {
        provider.models.retain(|model| beside.model.as_deref() == Some(model.model.id.as_str()));
        provider.image_models.clear();
        provider.video_models.clear();
        provider.audio_models.clear();
    }
    catalog
}

/// The plugins' page states the page reads: of the windows it opens alone.
/// Any other plugin's is its settings', as a sign-in's or a skill source's.
fn plugin_states(scope: &PageScope, states: BTreeMap<String, serde_json::Value>) -> BTreeMap<String, serde_json::Value> {
    states.into_iter().filter(|(plugin, _)| reads_state(scope, plugin)).collect()
}

fn reads_state(scope: &PageScope, plugin: &str) -> bool {
    window_operation(plugin).is_some_and(|operation| scope.allows(operation))
}

/// The blobs a page of each conversation uploaded, which it reads before a
/// message names them, as the composer's thumbnails do; kept while the
/// process runs.
#[derive(Default)]
pub struct PageUploads {
    uploaded: Mutex<HashMap<(UserId, ConversationId), BTreeSet<BlobRef>>>,
}

impl PageUploads {
    pub fn uploaded(&self, user: &UserId, conversation: &ConversationId, blob: BlobRef) {
        let mut uploaded = self.uploaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        uploaded.entry((user.clone(), conversation.clone())).or_default().insert(blob);
    }

    pub fn holds(&self, user: &UserId, conversation: &ConversationId, blob: &BlobRef) -> bool {
        let uploaded = self.uploaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        uploaded.get(&(user.clone(), conversation.clone())).is_some_and(|blobs| blobs.contains(blob))
    }
}

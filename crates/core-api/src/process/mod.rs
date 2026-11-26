//! A program's process, `jush-core`: it runs the cores its programs open,
//! and serves each program the cores' routes.
//!
//! A program reaches it in one of two ways, which serve the same routes:
//!
//! - It starts the process and speaks HTTP/2 over the process's standard
//!   input and output ([`run`]). The process ends with that connection,
//!   and every core closes.
//! - The process runs on its own, `jush-core serve`, until it is stopped
//!   ([`serve`]). Programs connect to it with its token, several at once,
//!   and a connection's end closes nothing; runners dial it as they dial a
//!   server.
//!
//! ```text
//! program --- HTTP/2 ---> PUT /plugins/{id} {manifest}  declares a plugin
//!                         DELETE /plugins/{id}          removes it
//!                         PUT, DELETE /plugins/{id}/versions/{version}
//!                                                       the same of one
//!                                                       version of it
//!                         POST /cores {dir}             opens a core: 201
//!                                                       {id}, or 200 for
//!                                                       one open already
//!                         DELETE /cores/{id}            closes it
//!                         /cores/{id}/api/...           the core's routes,
//!                                                       as a page has them
//!                                                       under /api
//!                         PUT /addresses {templates}    where the program
//!                                                       serves the calls
//!                                                       from outside
//!                         POST /schedules/calls?id=     a call of a
//!                                                       schedule's URL
//!                         POST /sign-ins                a sign-in's return
//! ```
//!
//! The process answers no one without its token but runners: the calls
//! from outside reach the program's own addresses, which hand them on
//! (`addresses.rs`).
//!
//! The process is an isolated instance with one set of services for its
//! cores, as the server has for its users'. A program is the caller of each
//! core's routes, and a starter: the core's notifications go on the newest
//! starter lane a program opened on its page connection to the core.

mod addresses;
mod cores;
mod devices;
mod listener;
mod packages;
mod plugins;
mod program;
mod runners;
mod stream;

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::extract::{FromRequestParts, OriginalUri, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, Uri};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, connect, delete, get, post, put};
use axum::{Extension, Json, Router};
use jush_core::builtins::{self, RelayFamily};
use jush_core::conversation::subscription_cli::MachineSlot;
use jush_core::pool::{ShardPlacement, ShardPool};
use jush_core::services::{CoreServices, CoreServicesError, CoreSettings};
use jush_core::tuning::{ConversationTuning, LifecycleTuning, MediaTuning, RunnerTuning};
use jush_core_blobs::blobs::BlobStores;
use jush_core_devices::claims::{PendingClaims, RecordedTokens};
use jush_core_devices::packages::ReleaseError;
use jush_core_devices::public_url::PublicUrl;
use jush_core_providers::vault::logins::LoginTiming;
use jush_provider_antigravity::releases::DEFAULT_RELEASES_URL as DEFAULT_ANTIGRAVITY_RELEASES_URL;
use jush_plugin_interface::Manifest;
use jush_provider_claude_code::releases::DEFAULT_RELEASES_URL;
use jush_provider_common::media_catalog;
use jush_provider_common::models_dev::ModelsDevClient;
use jush_shared_types::{Clock, SystemClock};
use jush_web_api_protocol::connection::CONNECT_ROUTE;
use jush_web_api_protocol::cores::{OpenCore, OpenedCore};
use jush_web_api_protocol::error::ErrorCode;
use jush_web_api_protocol::ids::{ConversationId, UserId};
use jush_web_api_protocol::settings::InstanceMode;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;
use tower_http::trace::TraceLayer;

pub use self::listener::serve;

use self::addresses::Addresses;
use self::cores::{OpenCores, ProcessOpener, not_open};
use self::packages::{ReleaseFiles, ServedPackages};
use self::plugins::ProgramPlugin;
use self::program::{PageCaller, ProgramCaller};
use crate::api::Api;
use crate::body::JsonBody;
use crate::plugin_lanes::{PluginVersion, undeclared};
use crate::caller::{Caller, CallerOf};
use crate::page::{OPERATIONS_HEADER, PageScope};
use crate::connection::{Closing, PageTuning};
use crate::error::ApiError;

/// Why the process did not run.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error(transparent)]
    Services(#[from] CoreServicesError),
    #[error("the cores' threads did not start: {0}")]
    Threads(std::io::Error),
    #[error(transparent)]
    Release(#[from] ReleaseError),
}

/// The process's cores and services, which its routes reach.
struct Process {
    cores: Arc<OpenCores>,
    api: Api,
    release: Arc<ReleaseFiles>,
    /// The core's routes, for a program's calls of a serving core.
    core_routes: Router,
    /// Where the programs serve the calls from outside.
    addresses: Arc<Addresses>,
    /// How long a request of a program's plugin waits for a lane of it.
    returning: Duration,
    closing: CancellationToken,
    /// The cores' threads, until the process ends.
    pool: Mutex<Option<ShardPool>>,
}

/// A request a program made, over the connection it gave the process or
/// with the process's token.
#[derive(Clone)]
struct Authorized;

impl<S: Send + Sync> FromRequestParts<S> for Authorized {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        parts.extensions.get::<Authorized>().cloned().ok_or_else(unauthorized)
    }
}

fn unauthorized() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        ErrorCode::Unauthenticated,
        "The call carries no valid token for this process",
    )
}

/// `PUT /plugins/{id}`: a plugin of a program's, its manifest.
#[derive(Debug, serde::Deserialize, garde::Validate)]
#[serde(transparent)]
struct DeclaredPlugin(#[garde(skip)] Manifest);

/// `version` of `/plugins/{id}/versions/{version}`, refused when it names
/// no version.
fn version_of(version: String) -> Result<String, ApiError> {
    if jush_web_api_protocol::is_plugin_version(&version) {
        Ok(version)
    } else {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::InvalidBody,
            format!("{version} is no plugin's version: 1 to 64 letters, digits, `.`, `_` and `-`"),
        ))
    }
}

/// Runs the process over `io`, the program's connection, until it ends,
/// then closes every core. `release` holds the roots of the release whose
/// command packages the cores' runners run; without a root, they run none.
pub async fn run<IO>(io: IO, release: Vec<PathBuf>) -> Result<(), ProcessError>
where
    IO: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let process = Process::start(release, false).await?;
    // Whatever comes over the connection is the program's.
    let app = routes(process.clone()).layer(Extension(Authorized));
    tracing::info!(event = "process.serving", "the process serves its program");
    let served = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
        .enable_connect_protocol()
        .serve_connection(TokioIo::new(io), TowerToHyperService::new(app))
        .await;
    if let Err(error) = served {
        tracing::warn!(event = "process.connection_failed", %error, "the program's connection failed");
    }
    process.close().await;
    tracing::info!(event = "process.ended", "the process ended with its program's connection");
    Ok(())
}

impl Process {
    /// Starts the process's services, with the release's plugins, which
    /// each program's declarations add to; `listens` for one on its own,
    /// which runners dial.
    async fn start(release: Vec<PathBuf>, listens: bool) -> Result<Arc<Self>, ProcessError> {
        let release = Arc::new(ReleaseFiles::read(release).await?);
        let cores = Arc::new(OpenCores::new());
        let addresses = Arc::new(Addresses::default());
        // The process is reached from outside at its programs' addresses
        // alone: it has no public URL, so its cores show no device a
        // command that dials it, and lend no entry under its name.
        let public = PublicUrl::default();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let subscriptions_machine = MachineSlot::default();
        let runners = RunnerTuning::default();
        let returning = runners.returning;
        let claims = Arc::new(PendingClaims::new(RecordedTokens, runners.claims_per_minute));
        // The process keeps no directory of its own, and several may run on
        // one machine: each takes a directory of its own for its plugins.
        let plugins = builtins::plugins(&std::env::temp_dir().join(format!("jush-core-{}", std::process::id())));
        let settings = CoreSettings {
            families: builtins::families(subscriptions_machine.clone())
                .with(jush_provider_relay::borrow::FAMILY, RelayFamily(public.clone())),
            public_url: public,
            clock: clock.clone(),
            plugins,
            packages: Arc::new(ServedPackages(release.clone())),
            models_dev_url: ModelsDevClient::DEFAULT_URL
                .parse()
                .expect("the models.dev address parses"),
            model_shutdowns_url: jush_provider_registry::shutdowns::DEFAULT_URL
                .parse()
                .expect("LiteLLM's address parses"),
            media_catalog_url: media_catalog::branch_url(option_env!("JUSH_BUILD_BRANCH")),
            media_catalog: media_catalog::Catalog::carried(),
            claude_releases: DEFAULT_RELEASES_URL
                .parse()
                .expect("the Claude Code distribution's address parses"),
            antigravity_releases: DEFAULT_ANTIGRAVITY_RELEASES_URL
                .parse()
                .expect("the Antigravity distribution's address parses"),
            subscriptions_machine,
            logins: LoginTiming::default(),
            runners,
            conversations: ConversationTuning::default(),
            media: MediaTuning::default(),
            lifecycle: LifecycleTuning::default(),
            stun: Vec::new(),
        };
        let services = CoreServices::start(
            settings,
            cores.clone(),
            InstanceMode::Isolated,
            false,
            BlobStores::own(clock),
        )
        .await?;
        let services = Arc::new(services);
        let opener = ProcessOpener {
            cores: cores.clone(),
            addresses: addresses.clone(),
            claims: listens.then(|| claims.clone()),
        };
        let pool = match ShardPool::start(
            ShardPlacement::Threads(NonZeroUsize::MIN),
            services.clone(),
            Arc::new(opener),
        )
        .await
        {
            Ok(pool) => pool,
            Err(error) => {
                services.close_providers().await;
                return Err(ProcessError::Threads(error));
            }
        };
        let api = Api {
            services,
            shards: pool.shards(),
            html: Arc::default(),
            page_uploads: Arc::default(),
            debug: Arc::default(),
            pages: PageTuning::default(),
            schedule_owners: cores.clone(),
            claims,
        };
        let closing = CancellationToken::new();
        let core_routes = core_routes(api.clone(), release.clone(), closing.clone());
        tracing::info!(event = "process.started", "the process's services started");
        Ok(Arc::new(Self {
            cores,
            api,
            release,
            core_routes,
            addresses,
            returning,
            closing,
            pool: Mutex::new(Some(pool)),
        }))
    }

    /// Closes every core, then the services; the runners waiting to be
    /// paired go.
    async fn close(&self) {
        self.closing.cancel();
        self.api.claims.close();
        self.cores.close_all(&self.api.services, &self.api.shards).await;
        let pool = self.pool.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(pool) = pool {
            pool.close().await;
        }
        self.api.services.close_providers().await;
    }

    /// Has every core take the programs' plugins as they are now, which a
    /// core that took them already does at once.
    async fn adopt_plugins(&self) {
        for id in self.cores.all_held() {
            // A core that closes meanwhile takes nothing more.
            let _ = self
                .api
                .shards
                .of(&id)
                .call(|core, _| async move { core.adopt_plugins().await })
                .await;
        }
    }
}

/// The core's routes, as a program calls them of a serving core, and as a
/// program's page calls them of one conversation, through the page's gate.
fn core_routes(api: Api, release: Arc<ReleaseFiles>, closing: CancellationToken) -> Router {
    crate::routes()
        .route("/state", get(crate::state::read))
        // The reads' caching (`revalidate.rs`): the routes' answers only.
        .layer(middleware::from_fn(crate::revalidate::reads))
        .route("/page", get(crate::page::answer))
        .route(
            CONNECT_ROUTE,
            any(crate::connection::connect).layer(Extension(Closing(closing))),
        )
        .merge(crate::public_routes())
        .route("/devices", post(devices::create))
        .route("/devices/{id}/runner", connect(devices::channel))
        .route_layer(middleware::from_fn(crate::page::gate))
        .fallback(no_route)
        .with_state(api)
        .layer(Extension(release))
        // Each stream is its request's connection.
        .layer(middleware::from_fn(stream::stream_peer))
}

/// The process's routes.
fn routes(process: Arc<Process>) -> Router {
    Router::new()
        .route("/plugins/{id}", put(declare_current).delete(remove_current))
        .route("/plugins/{id}/versions/{version}", put(declare_version).delete(remove_version))
        .route("/cores", post(open))
        .route("/cores/{id}", delete(close))
        .route("/addresses", put(addresses::declare))
        .route(
            "/schedules/calls",
            post(addresses::schedule_call).layer(axum::extract::DefaultBodyLimit::max(addresses::CALL_LIMIT)),
        )
        .route("/sign-ins", post(addresses::complete_sign_in))
        // A route with parameters would give them to the core's routes too.
        .fallback(core_call)
        .with_state(process)
        .layer(axum::extract::DefaultBodyLimit::max(crate::body::JSON_BODY_LIMIT))
        .layer(middleware::from_fn(crate::request_id::watch_abandon))
        // The request's line is `request_id::answered`'s alone.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request| crate::request_id::span(request))
                .on_response(crate::request_id::answered)
                .on_failure(()),
        )
        // Outermost, so every answer carries the request's ID.
        .layer(middleware::from_fn(crate::request_id::assign))
}

/// Runs `work`, a change a program asked of the process, to its end in a
/// task of its own, whether or not the program stays for the answer: a
/// program that goes away mid-way leaves no core half opened or half
/// closed, and no core behind the plugins the others took.
async fn to_the_end<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> T {
    match tokio::spawn(work).await {
        Ok(done) => done,
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    }
}

/// `PUT /plugins/{id}`: declares the program's plugin `id` as its current
/// one, in place of the one declared so before ([`declare`]).
async fn declare_current(
    State(process): State<Arc<Process>>,
    _: Authorized,
    Path(id): Path<String>,
    JsonBody(DeclaredPlugin(manifest)): JsonBody<DeclaredPlugin>,
) -> Result<StatusCode, ApiError> {
    declare(process, id, None, manifest).await
}

/// `PUT /plugins/{id}/versions/{version}`: declares the program's plugin
/// `id` as `version`, in place of the one declared so before ([`declare`]).
/// A process of a release before versions answers 404, as it has no such
/// route, and changes nothing.
async fn declare_version(
    State(process): State<Arc<Process>>,
    _: Authorized,
    Path((id, version)): Path<(String, String)>,
    JsonBody(DeclaredPlugin(manifest)): JsonBody<DeclaredPlugin>,
) -> Result<StatusCode, ApiError> {
    declare(process, id, Some(version_of(version)?), manifest).await
}

/// Declares `manifest`, the program's plugin `id`, as `version`, or as its
/// current one for none, 204; every open core takes it before the answer. A
/// manifest that breaks a rule, or takes the id of a plugin of the release,
/// is refused, 400.
async fn declare(process: Arc<Process>, id: String, version: Option<String>, manifest: Manifest) -> Result<StatusCode, ApiError> {
    if manifest.id.as_str() != id {
        return Err(ApiError::invalid_body(format!(
            "The manifest's id is \"{}\", not the plugin's in the path",
            manifest.id
        )));
    }
    let plugin = ProgramPlugin::new(manifest, version.clone(), process.cores.clone(), process.returning);
    process
        .api
        .services
        .plugins
        .declare(Arc::new(plugin), version.as_deref())
        .map_err(|error| ApiError::invalid_body(error.to_string()))?;
    to_the_end(async move { process.adopt_plugins().await }).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /plugins/{id}`: removes the program's plugin `id` as its current
/// one ([`remove`]).
async fn remove_current(State(process): State<Arc<Process>>, _: Authorized, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    remove(process, id, None).await
}

/// `DELETE /plugins/{id}/versions/{version}`: removes the program's plugin
/// `id` as `version` ([`remove`]).
async fn remove_version(
    State(process): State<Arc<Process>>,
    _: Authorized,
    Path((id, version)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    remove(process, id, Some(version_of(version)?)).await
}

/// Removes the program's plugin `id` as `version`, or as its current one
/// for none, the others staying, 204: its commands of that version leave
/// every open core before the answer, and its lanes of that version close;
/// 404 for a plugin no program declared so.
async fn remove(process: Arc<Process>, id: String, version: Option<String>) -> Result<StatusCode, ApiError> {
    let Some(plugin) = process.api.services.plugins.remove(&id, version.as_deref()) else {
        return Err(undeclared(&id, version.as_deref()));
    };
    let plugin = PluginVersion { plugin, version };
    to_the_end(async move {
        process.adopt_plugins().await;
        for core in process.cores.all_held() {
            if let Some(lanes) = process.cores.plugin_lanes(&core) {
                lanes.remove(&plugin);
            }
        }
    })
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /cores`: opens the core in `dir`, 201 with its id; 200 with it for
/// a core the process has open already.
async fn open(
    State(process): State<Arc<Process>>,
    _: Authorized,
    JsonBody(open): JsonBody<OpenCore>,
) -> Result<(StatusCode, Json<OpenedCore>), ApiError> {
    let (id, opened) = to_the_end(async move {
        let api = &process.api;
        process.cores.open(&PathBuf::from(open.dir), &api.services, &api.shards).await
    })
    .await?;
    let status = if opened { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(OpenedCore { id })))
}

/// `DELETE /cores/{id}`: closes the core for every program, 204.
async fn close(State(process): State<Arc<Process>>, _: Authorized, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    let id = UserId::try_from(id).map_err(|_| not_open())?;
    to_the_end(async move {
        let api = &process.api;
        process.cores.close(&id, &api.services, &api.shards).await
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `/cores/{id}/api/...`: a call of the serving core `id`, which the core's
/// routes answer as the server's answer a page's under `/api`.
///
/// `/cores/{id}/pages/{conversation}/...`: a call of a program's page of the
/// conversation, with the operations its program gave it in the
/// `jush-page-operations` header, which the same routes answer as the
/// page's (`crate::page`).
async fn core_call(State(process): State<Arc<Process>>, _: Authorized, mut request: Request) -> Result<Response, ApiError> {
    let Some((id, rest)) = request
        .uri()
        .path()
        .strip_prefix("/cores/")
        .and_then(|rest| rest.split_once('/'))
    else {
        return Ok(no_route(request).await);
    };
    let (place, bound) = if let Some(place) = rest.strip_prefix("api/") {
        (place, None)
    } else if let Some((conversation, place)) = rest.strip_prefix("pages/").and_then(|rest| rest.split_once('/')) {
        (place, Some(conversation.to_owned()))
    } else {
        return Ok(no_route(request).await);
    };
    let id = UserId::try_from(id.to_owned()).map_err(|_| not_open())?;
    let core = process.cores.serving(&id).ok_or_else(not_open)?;
    let path = match request.uri().query() {
        Some(query) => format!("/{place}?{query}"),
        None => format!("/{place}"),
    };
    *request.uri_mut() = path.parse::<Uri>().map_err(|_| not_open())?;
    let program = ProgramCaller::new(id, core, process.cores.clone());
    let caller: Arc<dyn Caller> = match bound {
        None => Arc::new(program),
        Some(conversation) => {
            let conversation = ConversationId::try_from(conversation).map_err(|_| {
                ApiError::new(StatusCode::NOT_FOUND, ErrorCode::ConversationNotFound, "Conversation not found")
            })?;
            let operations = request
                .headers()
                .get(OPERATIONS_HEADER)
                .map(|header| header.to_str().unwrap_or("\u{0}"))
                .unwrap_or_default();
            let scope = PageScope::new(conversation, PageScope::operations(operations)?);
            Arc::new(PageCaller::new(program, scope))
        }
    };
    request.extensions_mut().insert(CallerOf(caller));
    // The router answers every request.
    Ok(process.core_routes.clone().oneshot(request).await.unwrap_or_else(|never| match never {}))
}

pub(super) async fn no_route(request: Request) -> Response {
    // The core's routes see their own path; the program called the whole.
    let path = match request.extensions().get::<OriginalUri>() {
        Some(OriginalUri(uri)) => uri.path().to_owned(),
        None => request.uri().path().to_owned(),
    };
    ApiError::no_route(request.method(), &path).into_response()
}

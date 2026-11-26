//! `/api/devices`: the caller's devices.
//!
//! The routes:
//!
//! - pairing the runner waiting under the code it printed;
//! - renaming and routes, and revocation;
//! - browsing a paired device's directories to choose a target;
//! - a program's script on a device, outside any conversation.
//!
//! Browsing and a script reach the device through device access, which wakes
//! nothing: a device whose runner is not connected answers 409
//! `device_offline`.

use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use bytes::Bytes;
use jush_core_device_access::scripts::{self, Script, ScriptEnd};
use jush_core_devices::command_context::device_context;
use jush_device_interface::{JobCaller, OutputRecord, WholeOutput};
use jush_shared_types::{Labels, NodeId, StreamKind};
use jush_core_store::devices::{DeviceChange, DeviceRecord};
use jush_web_api_protocol::text::Trimmed;
use jush_core_devices::files::browse_directory;
use jush_device_interface::{DeviceError, DeviceErrorKind, MkdirOptions};
use jush_core_devices::claims::ClaimRefusal;
use jush_web_api_protocol::devices::{
    ChangeDevice, Claim, DeviceAnswer, DeviceElevation, DeviceExec, DeviceExecResult, DeviceKind,
    Devices, ExecOutput, RevokedDevice,
};
use jush_web_api_protocol::error::ErrorCode;
use jush_web_api_protocol::files::{
    CreateDeviceDirectory, CreatedDirectory, DeviceDirectoryQuery, Directory,
};
use jush_web_api_protocol::ids::DeviceId;
use jush_web_api_protocol::workflows::{KeptWorkflowList, WorkflowsQuery};
use jush_core::conversation::workflows::DeviceWorkflowsRefusal;

use crate::api::Api;
use crate::body::JsonBody;
use crate::error::ApiError;
use crate::caller::Principal;
use crate::query::QueryParams;

/// `POST /devices/claim`: pairs the runner waiting under the code with the
/// caller: the device is made, its token goes to that runner alone, and the
/// runner's socket moves into the caller's core.
pub(crate) async fn claim(
    State(state): State<Api>,
    Principal(user): Principal,
    JsonBody(Claim { code }): JsonBody<Claim>,
) -> Result<(StatusCode, Json<DeviceAnswer>), ApiError> {
    match state.claims.claim(&state.store(&user).await?, &user, &code).await {
        Ok(device) => Ok((StatusCode::CREATED, Json(DeviceAnswer { device }))),
        Err(ClaimRefusal::TooManyAttempts) => Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::RateLimited,
            "Too many claim attempts",
        )),
        Err(ClaimRefusal::UnknownCode) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::InvalidCode,
            "Unknown or expired pairing code",
        )),
        Err(ClaimRefusal::Storage(error)) => Err(error.into()),
    }
}

pub(crate) async fn list(
    State(state): State<Api>,
    Principal(user): Principal,
) -> Result<Json<Devices>, ApiError> {
    let devices = state
        .shards
        .of(&user)
        .call(|core, _| async move { core.device_list().await })
        .await??;
    let devices = devices
        .into_iter()
        .filter(|device| device.kind == DeviceKind::User)
        .collect();
    Ok(Json(Devices { devices }))
}

/// Renames a paired device, or sets its route; the Cloud keeps its name and
/// is always reached through the server.
pub(crate) async fn change(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
    JsonBody(ChangeDevice { name, route }): JsonBody<ChangeDevice>,
) -> Result<Json<DeviceAnswer>, ApiError> {
    let device = owned_device(&state, &user, &id, None).await?;
    if device.kind == DeviceKind::Managed {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::DeviceManaged,
            "The Cloud keeps its name and is always reached through the server",
        ));
    }
    let change = DeviceChange {
        name: name.map(Trimmed::into_string),
        route,
    };
    let changed = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.change_device(device.id, change).await })
        .await??;
    // A revocation between the lookup and the change took the device.
    let device = changed.ok_or_else(device_not_found)?;
    Ok(Json(DeviceAnswer { device }))
}

/// Revokes a paired device, which nothing refuses; its projects go with
/// it.
pub(crate) async fn revoke(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
) -> Result<Json<RevokedDevice>, ApiError> {
    let device = owned_device(&state, &user, &id, Some(DeviceKind::User)).await?;
    let removal = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.revoke_device(device.id).await })
        .await??;
    let removed = removal
        .projects
        .into_iter()
        .map(|project| project.id)
        .collect();
    Ok(Json(RevokedDevice { removed }))
}

/// Asks a paired device's Windows runner to run elevated, and answers once
/// the User Account Control prompt shows on its desktop, or could not; the
/// device's elevation says later what came of it. A device whose runner is
/// not connected answers 409 `device_offline`.
pub(crate) async fn elevate(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
) -> Result<Json<DeviceElevation>, ApiError> {
    let device = owned_device(&state, &user, &id, Some(DeviceKind::User)).await?;
    let answered = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.elevate_device(&device.id).await })
        .await?;
    match answered {
        Some(Ok(outcome)) => Ok(Json(DeviceElevation { outcome })),
        Some(Err(error)) => Err(ApiError::device_operation(error)),
        None => Err(ApiError::device_offline()),
    }
}

/// A directory of a paired device, its home when the query names none,
/// with the device's number before it listed, as the folder dialog's watch
/// compares it.
pub(crate) async fn browse(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
    QueryParams(query): QueryParams<DeviceDirectoryQuery>,
) -> Result<axum::response::Response, ApiError> {
    let device = owned_device(&state, &user, &id, Some(DeviceKind::User)).await?;
    let requested = query.path.map(|path| path.as_str().to_owned());
    let listed = state
        .shards
        .of(&user)
        .call(move |core, _| async move {
            let remote = core
                .devices()
                .device_access(&device.id)
                .await
                .ok_or_else(ApiError::device_offline)?;
            let home = core.devices().home(&device.id);
            let path = requested
                .or_else(|| home.clone())
                .ok_or_else(|| ApiError::invalid_query("Missing path query parameter"))?;
            let (entries, read_at) = browse_directory(&remote, &path)
                .await
                .map_err(device_fs_error)?;
            let directory = Directory {
                path,
                home,
                entries,
            };
            Ok::<_, ApiError>((directory, read_at))
        })
        .await??;
    let (directory, read_at) = listed;
    Ok(crate::files::read_answer(&read_at, Json(directory)))
}

/// `POST /devices/:id/exec`: runs a script on a paired device, outside any
/// conversation, as the agent's shell runs a command, and answers once it
/// ended. A caller that goes away first, as a program whose connection ends,
/// has it stopped. The core's close answers at once; the device stops the
/// script when it connects again (`crates/runner-client/src/device_jobs.rs`).
pub(crate) async fn exec(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
    JsonBody(exec): JsonBody<DeviceExec>,
) -> Result<Json<DeviceExecResult>, ApiError> {
    let device = owned_device(&state, &user, &id, Some(DeviceKind::User)).await?;
    state
        .shards
        .of(&user)
        .call(move |core, gone| async move {
            let remote = core
                .devices()
                .device_access(&device.id)
                .await
                .ok_or_else(ApiError::device_offline)?;
            let script = Script {
                script: exec.script,
                cwd: exec.cwd.or_else(|| core.devices().home(&device.id)),
                env: exec.env,
                stdin: Some(Bytes::from(exec.stdin)),
                context: device_context(core.store()).await?,
                caller: JobCaller {
                    node: NodeId::try_from("program").expect("a name is not empty"),
                    tool_use_id: "exec".into(),
                    turn: None,
                    labels: Labels::new(),
                    assignment_id: None,
                },
            };
            let time = exec.timeout_ms.map(Duration::from_millis);
            let began = Instant::now();
            let stop = gone.child_token();
            let run = scripts::run(&remote, script, time, &stop);
            tokio::pin!(run);
            let ran = tokio::select! {
                ran = &mut run => ran,
                () = core.closed() => {
                    stop.cancel();
                    run.await
                }
            };
            // A script that never started leaves the device as it was.
            let ran = ran.map_err(|error| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    ErrorCode::DeviceOffline,
                    format!("The script did not start: {}", error.message),
                )
            })?;
            tracing::info!(
                event = "device_exec.ended",
                device_id = %device.id,
                end = ?ran.end,
                elapsed_ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX),
                "a program's script on a device ended"
            );
            let output = exec_output(&ran.output);
            let timeout_ms = exec.timeout_ms.unwrap_or_default();
            match ran.end {
                ScriptEnd::Exited(exit_code) => Ok(Json(DeviceExecResult { exit_code, output })),
                // What the script did is the caller's, not the backend's: no
                // 5xx, which the backend's log counts as its own failure.
                ScriptEnd::TimedOut { stopped: true } => Err(ApiError::new(
                    StatusCode::CONFLICT,
                    ErrorCode::CommandTimedOut,
                    format!("The script ran past {timeout_ms} ms and was stopped"),
                )
                .with_output(output)),
                ScriptEnd::TimedOut { stopped: false } => Err(ApiError::new(
                    StatusCode::CONFLICT,
                    ErrorCode::DeviceLost,
                    format!("The script ran past {timeout_ms} ms while the device's runner was away"),
                )),
                ScriptEnd::Lost(reason) => Err(ApiError::new(
                    StatusCode::CONFLICT,
                    ErrorCode::DeviceLost,
                    format!("The device lost the script: {reason}"),
                )),
                // The core closes, or the caller went away and reads no answer.
                ScriptEnd::Stopped => Err(ApiError::backend_closing()),
            }
        })
        .await?
}

/// A script's output as the contract carries it: each stream's bytes read as
/// UTF-8, and the bytes the device left out or that were not read from it.
fn exec_output(output: &WholeOutput) -> ExecOutput {
    let mut streams = [Vec::new(), Vec::new()];
    let mut left_out = output.missing().map_or(0, |missing| missing.bytes);
    for record in output.records() {
        match record {
            OutputRecord::Output(StreamKind::Stdout, bytes) => streams[0].extend_from_slice(bytes),
            OutputRecord::Output(StreamKind::Stderr, bytes) => streams[1].extend_from_slice(bytes),
            OutputRecord::LeftOut(bytes) => left_out += bytes,
        }
    }
    let [stdout, stderr] = streams.map(|bytes| {
        String::from_utf8(bytes).unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
    });
    ExecOutput {
        stdout,
        stderr,
        left_out_bytes: left_out,
    }
}

/// `GET /devices/:id/workflows?directory=`: the Workflow menu's list for a new
/// conversation that works in `directory` of a paired device, before its first
/// message gives it a record
/// (`crates/core/src/conversation/workflows.rs`).
pub(crate) async fn workflows(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
    query: Result<QueryParams<WorkflowsQuery>, ApiError>,
) -> Result<Json<KeptWorkflowList>, ApiError> {
    // A Cloud project's are read as a paired device's, through device
    // access, which wakes no stopped Cloud.
    let device = owned_device(&state, &user, &id, None).await?;
    // A directory that is not an absolute path is the request's fault, as a
    // body's would be.
    let QueryParams(query) = query.map_err(|error| ApiError::invalid_body(error.message().to_owned()))?;
    let workflows = state
        .shards
        .of(&user)
        .call(move |core, _| async move {
            core
                .device_workflows(&device.id, query.directory.as_str())
                .await
                .map_err(|refusal| match refusal {
                    DeviceWorkflowsRefusal::Offline => ApiError::device_offline(),
                    DeviceWorkflowsRefusal::NoDirectory => ApiError::new(
                        StatusCode::NOT_FOUND,
                        ErrorCode::FsNotFound,
                        "No such directory on the device",
                    ),
                    DeviceWorkflowsRefusal::Device(error) => device_fs_error(error),
                    DeviceWorkflowsRefusal::Storage(error) => ApiError::from(error),
                })
        })
        .await??;
    Ok(Json(workflows))
}

/// Makes a directory, with its parents, on a paired device.
pub(crate) async fn make_directory(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
    JsonBody(CreateDeviceDirectory { path }): JsonBody<CreateDeviceDirectory>,
) -> Result<(StatusCode, Json<CreatedDirectory>), ApiError> {
    let device = owned_device(&state, &user, &id, Some(DeviceKind::User)).await?;
    let path = path.as_str().to_owned();
    let made = state
        .shards
        .of(&user)
        .call(move |core, _| async move {
            let device = core
                .devices()
                .device_access(&device.id)
                .await
                .ok_or_else(ApiError::device_offline)?;
            let recursive = MkdirOptions { recursive: true };
            jush_device_interface::DeviceFs::mkdir(&device, &path, recursive)
                .await
                .map_err(device_fs_error)?;
            Ok::<_, ApiError>(CreatedDirectory { path })
        })
        .await??;
    Ok((StatusCode::CREATED, Json(made)))
}

/// The caller's device `id`, of `kind` when one is named; 404 otherwise.
pub(crate) async fn owned_device(
    state: &Api,
    user: &jush_web_api_protocol::ids::UserId,
    id: &str,
    kind: Option<DeviceKind>,
) -> Result<DeviceRecord, ApiError> {
    let id = DeviceId::try_from(id).map_err(|_| device_not_found())?;
    let device = state
        .store(user)
        .await?
        .device(id)
        .await?
        .ok_or_else(device_not_found)?;
    if !device.is_device() || kind.is_some_and(|kind| kind != device.kind) {
        return Err(device_not_found());
    }
    tracing::Span::current().record("device_id", tracing::field::display(&device.id));
    Ok(device)
}

fn device_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        ErrorCode::DeviceNotFound,
        "No such device",
    )
}

/// A device route's filesystem failure: nothing at the path answers 404,
/// anything else the device refused 400.
fn device_fs_error(error: DeviceError) -> ApiError {
    match error.kind {
        DeviceErrorKind::Offline => ApiError::device_offline(),
        _ if error.code() == Some("ENOENT") => {
            let gone = ApiError::new(StatusCode::NOT_FOUND, ErrorCode::FsError, error.message);
            // Nothing there is an answer, read at a number like any other.
            match error.read_at {
                Some(read_at) => gone.with_read_at(read_at),
                None => gone,
            }
        }
        _ => ApiError::new(StatusCode::BAD_REQUEST, ErrorCode::FsError, error.message),
    }
}

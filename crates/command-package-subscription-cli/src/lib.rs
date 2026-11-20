//! The `jush.subscription-cli` command package: Jush's own verified copy of
//! each subscription's CLI, such as Claude Code, on the machine that runs it,
//! which the runner installs.
//!
//! It is a package of its own. The runner knows nothing about any CLI, and
//! removing the package and the backend's use of it leaves the rest of Jush as
//! it was. The package knows no CLI either: a release record names the CLI,
//! and each CLI's versions are a line of their own in the runner's cache.
//!
//! # Rules
//!
//! - The CLI is an artifact of the package, installed as every artifact is,
//!   through the runner's cache. There is no fallback to another version,
//!   location or executable.
//! - The package picks the platform from the machine, not from the runner's own
//!   target, because a runner built for musl runs on glibc machines and a
//!   CLI's `linux-*` and `linux-*-musl` builds are not interchangeable. The
//!   platform keys are Jush's, which the backend reads each vendor's into. A
//!   platform the release does not carry is `unsupported_platform`, never
//!   another platform's build.
//! - It reads no pointers and chooses no versions: the backend builds the
//!   release record, and Jush is the only thing that changes the CLI's version
//!   (the CLI's own updater is off).
//! - It starts no CLI, and is never linked into the runner or another command
//!   program.

pub mod install;
pub mod platform;
pub mod release;

use std::{future::Future, pin::Pin};

use bytes::Bytes;
use jush_command_package_subscription_cli_protocol::{Failure, Installed, Operation, Reply, Status};
use jush_command_protocol::{CommandError, Completion, Invocation};
use jush_command_sdk::{Artifacts, Handler, Input, InvocationContext, ResidentError};
use serde::Serialize;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use install::OperationError;

/// The most bytes of input read: the input is held whole in memory, and a
/// release record is only a few hundred bytes for each platform.
const MAX_INPUT_BYTES: usize = 64 * 1024;

/// The resident holds no conversation state, so the trait's empty conversation
/// status and its `close` stand.
pub struct JushSubscriptionCli {
    /// The resident's artifacts source, which it takes before it admits a
    /// call.
    artifacts: watch::Sender<Option<Artifacts>>,
}

impl Default for JushSubscriptionCli {
    fn default() -> Self {
        Self {
            artifacts: watch::Sender::new(None),
        }
    }
}

impl Handler for JushSubscriptionCli {
    type Metadata = Invocation;

    fn artifacts(&self, artifacts: Artifacts) {
        self.artifacts.send_replace(Some(artifacts));
    }

    fn operations(&self) -> Vec<String> {
        Operation::ALL
            .into_iter()
            .map(|operation| operation.name().to_owned())
            .collect()
    }

    fn invoke(
        &self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<Completion, ResidentError>> + Send>> {
        let artifacts = self.artifacts.borrow().clone();
        Box::pin(async move {
            let InvocationContext {
                request,
                input,
                output,
                cancellation,
            } = context;
            let Some(operation) = Operation::parse(&request.operation) else {
                return Err(ResidentError::UnknownOperation(request.operation));
            };
            let result = match artifacts {
                None => Err(OperationError::InstallFailed(
                    "jush-subscription-cli has no artifacts source".into(),
                )),
                Some(artifacts) => match operation {
                    Operation::Ensure => {
                        ensure(&artifacts, &request.invocation_id, input, &cancellation)
                            .await
                            .and_then(document)
                    }
                    Operation::Status => status(&artifacts, input, &cancellation)
                        .await
                        .and_then(document),
                },
            };
            let (body, completion) = match result {
                Ok(body) => (
                    body,
                    Completion {
                        exit_code: 0,
                        error: None,
                    },
                ),
                Err(error) => {
                    // Only a cancellation has no code; it writes no document.
                    let Some(code) = error.code() else {
                        return Err(ResidentError::Cancelled);
                    };
                    let message = error.to_string();
                    let body = serde_json::to_vec(&Reply::<()>::Failed(Failure {
                        code,
                        message: message.clone(),
                    }))?;
                    (
                        body,
                        Completion {
                            exit_code: 1,
                            error: Some(CommandError {
                                code: code.to_string(),
                                message,
                            }),
                        },
                    )
                }
            };
            output.stdout(line(body)).await?;
            Ok(completion)
        })
    }
}

async fn ensure(
    artifacts: &Artifacts,
    invocation: &str,
    input: Input,
    cancel: &CancellationToken,
) -> Result<Installed, OperationError> {
    let input = read_input(input, cancel, OperationError::InvalidRelease).await?;
    let release = install::release(&input)?;
    tokio::select! {
        _ = cancel.cancelled() => Err(OperationError::Cancelled),
        installed = install::ensure(artifacts, invocation, &release) => installed,
    }
}

async fn status(
    artifacts: &Artifacts,
    input: Input,
    cancel: &CancellationToken,
) -> Result<Status, OperationError> {
    let input = read_input(input, cancel, OperationError::InvalidRequest).await?;
    let request = install::status_request(&input)?;
    install::status(artifacts, &request.cli).await
}

/// Read the invocation's input to its end, refusing more than
/// `MAX_INPUT_BYTES`; `invalid` says why the input is refused.
async fn read_input(
    mut input: Input,
    cancel: &CancellationToken,
    invalid: fn(String) -> OperationError,
) -> Result<Vec<u8>, OperationError> {
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(OperationError::Cancelled),
            chunk = input.next() => chunk,
        };
        match chunk {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= MAX_INPUT_BYTES => {
                bytes.extend_from_slice(&chunk);
            }
            Ok(Some(_)) => return Err(invalid(format!("input exceeds {MAX_INPUT_BYTES} bytes"))),
            Ok(None) => return Ok(bytes),
            Err(ResidentError::Cancelled) => return Err(OperationError::Cancelled),
            Err(error) => return Err(invalid(error.to_string())),
        }
    }
}

fn document<T: Serialize>(answer: T) -> Result<Vec<u8>, OperationError> {
    serde_json::to_vec(&Reply::Done(answer))
        .map_err(|error| OperationError::InstallFailed(error.to_string()))
}

fn line(mut body: Vec<u8>) -> Bytes {
    body.push(b'\n');
    Bytes::from(body)
}

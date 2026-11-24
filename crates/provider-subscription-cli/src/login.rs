//! A subscription's sign-in through its CLI's own login, the same for every
//! CLI but in what [`CliLogin`] says.
//!
//! Jush runs the CLI's login on the Subscriptions machine, in a new
//! configuration directory, shows the user the link the CLI prints, and writes
//! the code the user pastes to the CLI's input.
//!
//! - When the CLI exits with success, Jush reads the account it signed in,
//!   and nothing of its tokens, and the directory becomes the account's.
//! - When the login fails or is stopped, the directory goes.
//! - A code a CLI refuses while it waits for another leaves it waiting, and
//!   the user sees its words and pastes another. A CLI that ends on a refused
//!   code ends the sign-in with its words.
//!
//! A CLI that can also sign in with a refresh token another app of the
//! vendor's holds starts a process that reads the token from its input, as
//! the CLI's login reads a code, makes the CLI's sign-in of it and has the
//! CLI renew it. The token is the vendor's to judge: a CLI that prints its
//! sign-in link instead has been refused it, and the sign-in ends.
//!
//! Jush never writes a vendor's OAuth flow itself, and never logs the code
//! or the token.

use std::sync::Arc;

use bytes::Bytes;
use futures_util::future::{BoxFuture, LocalBoxFuture};
use futures_util::stream::LocalBoxStream;
use futures_util::{FutureExt as _, StreamExt as _};
use jush_device_interface::{Process, ProcessControl, ProcessEnd, ProcessOutput};
use jush_provider_common::Secret;
use jush_provider_common::credentials::{
    AccountKit, AccountLabel, AccountsCapability, AccountsError, CredentialPool, LoginError,
    LoginIo, LoginKind, NewAccount, credential_id_for, find_by_identity,
};
use jush_shared_types::{DownloadProgress, LoginPending, LoginStage, RefusedCode, StreamKind};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::process::{HeldProcess, Tail, exit_message};
use crate::{
    AccountMachine, AccountWork, Cli, CliSite, CliStart, CliStarter, CliWork, ConfigDir, Placed,
    Placement, StartStage,
};

/// What one CLI's login differs in.
pub trait CliLogin: Send + Sync + 'static {
    /// The CLI.
    fn cli(&self) -> &'static Cli;

    /// The request that starts the CLI's own sign-in at `site`, in its new
    /// configuration directory. It prints the link to sign in at, reads the
    /// code the page shows from its input, and opens no web browser.
    fn request(&self, site: &CliSite) -> CliStart;

    /// How the CLI signs in with a refresh token: the request that starts it
    /// at a site, in its new configuration directory, which reads the token
    /// from its input and ends once the CLI renewed it, or prints the CLI's
    /// sign-in link when the vendor refuses it. None for a CLI that cannot.
    fn refresh_token_request(&self) -> Option<fn(&CliSite) -> CliStart> {
        None
    }

    /// Whether `line`, a line the CLI wrote after its link, refuses the code
    /// the user pasted while the CLI waits for another. A CLI that ends on a
    /// code it refuses has none.
    fn refuses(&self, line: &str) -> bool {
        let _ = line;
        false
    }

    /// The stream on which the CLI says why its sign-in failed, read first;
    /// the other stream says it when this one said nothing.
    fn says_failure_on(&self) -> StreamKind;

    /// The account the sign-in wrote into its directory `config`, read from
    /// the CLI's files without its tokens. Its identity key names the account
    /// in every family's, since the key names the account's directory and
    /// every CLI's accounts share a parent directory, as Claude Code's
    /// `account:<ID>` does.
    fn account<'a>(
        &'a self,
        config: &'a dyn ConfigDir,
    ) -> LocalBoxFuture<'a, Result<AccountLabel, LoginError>>;

    /// The account `id` signed in, with what the CLI fetched for it then.
    fn signed_in(&self, id: &str) {
        let _ = id;
    }
}

/// What a CLI's family adds to the account operations: the CLI's own
/// sign-in, on the machine account work runs on. A kit without a machine, as
/// of a provider built for inference, cannot sign in.
pub struct CliKit {
    /// What the CLI's login differs in.
    pub login: Arc<dyn CliLogin>,
    pub machine: Option<Arc<dyn AccountMachine>>,
    /// The entry the sign-in is for, or the login that is to make it.
    pub entry: String,
    /// The entry's accounts, which name the directory a sign-in's account
    /// takes.
    pub pool: Arc<dyn CredentialPool>,
}

impl AccountKit for CliKit {
    fn capability(&self) -> AccountsCapability {
        AccountsCapability {
            login: self.machine.is_some().then_some(LoginKind::PastedCode),
        }
    }

    fn login<'a>(
        &'a self,
        io: LoginIo<'a>,
    ) -> Option<BoxFuture<'a, Result<NewAccount, LoginError>>> {
        let machine = self.machine.clone()?;
        let request: CliStarter = match io.kind {
            LoginKind::RefreshToken => {
                let Some(request) = self.login.refresh_token_request() else {
                    return Some(Box::pin(async { Err(LoginError::Unsupported) }));
                };
                Arc::new(request)
            }
            LoginKind::PastedCode | LoginKind::Device => {
                let login = self.login.clone();
                Arc::new(move |site: &CliSite| login.request(site))
            }
        };
        Some(Box::pin(sign_in(
            machine,
            self.login.clone(),
            request,
            self.entry.clone(),
            self.pool.clone(),
            io,
        )))
    }

    /// The account's directory on the machine goes before its record, so
    /// no sign-in outlives the account in Jush.
    fn forget<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), AccountsError>> {
        Box::pin(async move {
            let machine = self.machine.clone().ok_or_else(|| {
                AccountsError::Unavailable("The Subscriptions machine is not available".into())
            })?;
            machine
                .remove_account(id.to_owned())
                .await
                .map_err(|error| AccountsError::Unavailable(error.0))
        })
    }
}

/// What the sign-in shows the user while it runs.
enum Shown {
    /// What the sign-in waits for before the CLI prints its link.
    Stage(LoginStage),
    /// How far the CLI's download came while it installs.
    Download(DownloadProgress),
    /// The link the CLI printed.
    Link(String),
    /// The process that takes a refresh token started, and waits for it.
    WaitingForToken,
    /// The CLI refused a code the user pasted, and waits for another.
    CodeRefused(RefusedCode),
}

/// Runs the CLI's sign-in on `machine`, which `request` starts, and relays
/// it.
///
/// What the start waits for goes to `io.stage`, the link the CLI prints to
/// `io.pending`, and the codes the user pastes go to the CLI. It ends with the
/// account the CLI signed in, or once the sign-in failed or was stopped, after
/// the sign-in's directory went.
async fn sign_in(
    machine: Arc<dyn AccountMachine>,
    login: Arc<dyn CliLogin>,
    request: CliStarter,
    entry: String,
    pool: Arc<dyn CredentialPool>,
    io: LoginIo<'_>,
) -> Result<NewAccount, LoginError> {
    let LoginIo {
        kind,
        pending,
        stage,
        download,
        codes,
        stop,
    } = io;
    let cli = login.cli();
    let (shown_to, mut link) = mpsc::unbounded_channel();
    let (answer, answered) = oneshot::channel();
    let work: AccountWork = {
        let login = login.clone();
        Box::new(move |placement, abandoned| {
            Box::pin(async move {
                let adopted = sign_in_at(
                    &*placement,
                    login,
                    request,
                    kind == LoginKind::RefreshToken,
                    &*pool,
                    shown_to,
                    codes,
                    stop,
                    abandoned,
                )
                .await;
                // A login that gave up waiting has stopped the sign-in already.
                let _ = answer.send(adopted);
            })
        })
    };
    let ran = machine.run(cli, entry, work);
    tokio::pin!(ran);
    let mut link_url = None;
    let mut show = |next: Shown| match next {
        Shown::Stage(reached) => stage(reached),
        Shown::Download(progress) => download(progress),
        Shown::Link(url) => {
            pending(LoginPending {
                verification_url: Some(url.clone()),
                user_code: None,
                code_error: None,
            });
            link_url = Some(url);
        }
        Shown::WaitingForToken => pending(LoginPending {
            verification_url: None,
            user_code: None,
            code_error: None,
        }),
        Shown::CodeRefused(refused) => {
            // The CLI reads codes only after it printed its link.
            if let Some(url) = &link_url {
                pending(LoginPending {
                    verification_url: Some(url.clone()),
                    user_code: None,
                    code_error: Some(refused),
                });
            }
        }
    };
    let mut shown = true;
    let ran = loop {
        tokio::select! {
            biased;
            next = link.recv(), if shown => match next {
                Some(next) => show(next),
                None => shown = false,
            },
            ran = &mut ran => break ran,
        }
    };
    // What the sign-in reported before it ended is shown first, in its
    // order, so that its outcome names the stage it reached.
    while let Ok(next) = link.try_recv() {
        show(next);
    }
    ran.map_err(|error| LoginError::Failed(error.0))?;
    let (id, account) = answered.await.unwrap_or_else(|_| {
        Err(LoginError::Failed(format!(
            "The {} sign-in ended without an answer",
            cli.name
        )))
    })?;
    login.signed_in(&id);
    Ok(account)
}

/// The sign-in on the machine `placement` stands for, which `request` starts,
/// whose account takes its id among `pool`'s. `with_token` is whether the
/// process takes a refresh token rather than printing a link for a code.
///
/// It ends early when `stop` fires or nobody waits for it any more
/// (`abandoned`), and removes its directory however it ends but with success,
/// which makes the directory the account's.
async fn sign_in_at(
    placement: &dyn Placement,
    login: Arc<dyn CliLogin>,
    request: CliStarter,
    with_token: bool,
    pool: &dyn CredentialPool,
    shown: mpsc::UnboundedSender<Shown>,
    codes: mpsc::UnboundedReceiver<Secret>,
    stop: CancellationToken,
    abandoned: CancellationToken,
) -> Result<(String, NewAccount), LoginError> {
    let stopped = || async {
        tokio::select! {
            () = stop.cancelled() => {}
            () = abandoned.cancelled() => {}
        }
    };
    // The login is gone once it stopped waiting, and a stage nobody shows
    // matters to nobody.
    let _work = placement.work();
    let stages = {
        let shown = shown.clone();
        move |reached: StartStage| {
            let _ = shown.send(match reached {
                StartStage::Machine => Shown::Stage(LoginStage::WaitingForMachine),
                StartStage::Install => Shown::Stage(LoginStage::Installing),
                StartStage::Download(progress) => Shown::Download(progress),
                StartStage::Process => Shown::Stage(LoginStage::Starting),
            });
        }
    };
    // A start given up removes what it made.
    let Placed {
        process, config, ..
    } = tokio::select! {
        placed = placement.start(CliWork::Login, request, &stages) => {
            placed.map_err(|error| LoginError::Failed(error.0))?
        }
        () = stopped() => return Err(LoginError::Stopped),
    };
    let Process {
        output,
        control,
        exit,
    } = process;
    let exit = exit.shared();
    // The process is held apart from what stops, and closed, SIGTERM
    // first, unless it ended by itself.
    let mut held = HeldProcess::of(control, exit.clone(), None);
    if with_token {
        // As above.
        let _ = shown.send(Shown::WaitingForToken);
    }
    let relayed = tokio::select! {
        relayed = relay(output, held.control(), &*login, with_token, shown, codes) => relayed,
        () = stopped() => Err(LoginError::Stopped),
    };
    let ended = match relayed {
        Ok(said) => tokio::select! {
            () = held.ended() => match exit.clone().await {
                ProcessEnd::Exited(0) => Ok(()),
                // A CLI that exits says why; one that was ended, or whose
                // machine went away, did not get to.
                ProcessEnd::Exited(_) if !said.is_empty() => Err(LoginError::Failed(said)),
                end => Err(LoginError::Failed(exit_message(login.cli(), &end))),
            },
            () = stopped() => Err(LoginError::Stopped),
        },
        Err(error) => Err(error),
    };
    held.close().await;
    if let Err(error) = ended {
        config.remove().await;
        return Err(error);
    }
    // The adoption waits for the account's processes, and stops with the
    // login all the same; one given up removes the sign-in's directory.
    tokio::select! {
        adopted = adopt(config, &*login, pool) => adopted,
        () = stopped() => Err(LoginError::Stopped),
    }
}

/// Relays the sign-in until the CLI's output ends.
///
/// The link it prints goes to `shown`, and each code to its input, followed by
/// a newline. A code the CLI refuses while it waits for another goes to
/// `shown` in the CLI's words. Answers what the CLI said last, on the stream
/// it says a failure on, else on the other: the tail of its standard error,
/// or its last line of standard output other than its link.
///
/// `with_token`: the input takes a refresh token, and a link the CLI prints,
/// on either stream, is its sign-in asked for once the vendor refused the
/// token, which ends the sign-in.
async fn relay(
    mut output: LocalBoxStream<'static, ProcessOutput>,
    control: &dyn ProcessControl,
    login: &dyn CliLogin,
    with_token: bool,
    shown: mpsc::UnboundedSender<Shown>,
    mut codes: mpsc::UnboundedReceiver<Secret>,
) -> Result<String, LoginError> {
    let mut stdout = Vec::new();
    let mut last_line = String::new();
    let mut stderr = Tail::default();
    let mut stderr_line = Vec::new();
    let mut linked = false;
    let mut coded = false;
    // The CLI judges the codes in the order it reads them, one refusal
    // each, so the nth refusal is of the nth code.
    let mut refused = 0;
    loop {
        tokio::select! {
            chunk = output.next() => {
                let Some(chunk) = chunk else {
                    // Output that ends without a newline ends its last line.
                    let rest = String::from_utf8_lossy(&stdout);
                    if !rest.trim().is_empty() {
                        last_line = rest.trim().to_owned();
                    }
                    let (first, second) = match login.says_failure_on() {
                        StreamKind::Stdout => (last_line, stderr.text()),
                        StreamKind::Stderr => (stderr.text(), last_line),
                    };
                    return Ok(if first.is_empty() { second } else { first });
                };
                match chunk.stream {
                    StreamKind::Stdout => {
                        stdout.extend_from_slice(&chunk.bytes);
                        while let Some(end) = stdout.iter().position(|byte| *byte == b'\n') {
                            let line: Vec<u8> = stdout.drain(..=end).collect();
                            let line = String::from_utf8_lossy(&line).trim().to_owned();
                            if with_token && link(&line).is_some() {
                                return Err(token_refused(login));
                            }
                            if !linked && let Some(url) = link(&line) {
                                linked = true;
                                // The login is gone once it stopped waiting.
                                let _ = shown.send(Shown::Link(url));
                                continue;
                            }
                            if line.is_empty() {
                                continue;
                            }
                            if linked && login.refuses(&line) {
                                refused += 1;
                                // As above.
                                let _ = shown.send(Shown::CodeRefused(RefusedCode {
                                    code: refused,
                                    message: line.clone(),
                                }));
                            }
                            last_line = line;
                        }
                    }
                    StreamKind::Stderr => {
                        stderr.push(&chunk.bytes);
                        stderr_line.extend_from_slice(&chunk.bytes);
                        while let Some(end) = stderr_line.iter().position(|byte| *byte == b'\n') {
                            let line: Vec<u8> = stderr_line.drain(..=end).collect();
                            let line = String::from_utf8_lossy(&line).trim().to_owned();
                            if with_token && link(&line).is_some() {
                                return Err(token_refused(login));
                            }
                            if login.refuses(&line) {
                                refused += 1;
                                // As above.
                                let _ = shown.send(Shown::CodeRefused(RefusedCode {
                                    code: refused,
                                    message: line,
                                }));
                            }
                        }
                    }
                }
            }
            code = codes.recv(), if !coded => {
                let Some(code) = code else {
                    coded = true;
                    continue;
                };
                let mut line = code.expose().as_bytes().to_vec();
                line.push(b'\n');
                control.write_stdin(Bytes::from(line)).await.map_err(|error| {
                    let what = if with_token { "refresh token" } else { "code" };
                    LoginError::Failed(format!(
                        "The {what} could not be given to {}: {error}",
                        login.cli().name
                    ))
                })?;
            }
        }
    }
}

/// Why a sign-in with a refresh token ended once the CLI printed its sign-in
/// link, on either stream: the vendor refused the token.
fn token_refused(login: &dyn CliLogin) -> LoginError {
    LoginError::Failed(format!("{} did not accept this refresh token.", login.cli().name))
}

/// The sign-in link a line of the CLI's output names: its first `https`
/// address, inside a terminal hyperlink when the CLI wrote one.
fn link(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|character: char| character.is_whitespace() || character.is_control())
        .unwrap_or(rest.len());
    let url = url::Url::parse(&rest[..end]).ok()?;
    (url.scheme() == "https").then(|| url.to_string())
}

/// The account the CLI's sign-in wrote into its directory `config`, which
/// becomes the account's directory, with its id.
///
/// The account's id is the one it has among `pool`'s accounts, or the one a
/// new account gets.
async fn adopt(
    config: Box<dyn ConfigDir>,
    login: &dyn CliLogin,
    pool: &dyn CredentialPool,
) -> Result<(String, NewAccount), LoginError> {
    let label = match login.account(&*config).await {
        Ok(label) => label,
        Err(error) => {
            config.remove().await;
            return Err(error);
        }
    };
    let existing = match &label.identity_key {
        Some(key) => find_by_identity(pool, key).await,
        None => Ok(None),
    };
    let id = match existing {
        Ok(Some(existing)) => existing.id,
        Ok(None) => credential_id_for(label.identity_key.as_deref(), &label.label),
        Err(error) => {
            config.remove().await;
            return Err(LoginError::Failed(error.to_string()));
        }
    };
    config.adopt(&id).await.map_err(|error| {
        LoginError::Failed(format!(
            "The {} sign-in could not become the account's: {error}",
            login.cli().name
        ))
    })?;
    Ok((
        id,
        NewAccount {
            label,
            secret: None,
        },
    ))
}

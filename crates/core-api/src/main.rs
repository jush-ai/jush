//! `jush-core`: the process programs run Jush's cores in
//! (`crates/core-api/src/process/mod.rs`).
//!
//! - `jush-core`: a program starts it and speaks HTTP/2 to it over its
//!   standard input and output. It ends once the program's connection does.
//! - `jush-core serve --listen <address>`: it runs on its own, and programs
//!   and runners connect to it over TCP at `address`, such as
//!   `127.0.0.1:4100`, until it is stopped with `SIGTERM` or Ctrl-C.
//!   `JUSH_CORE_TOKEN` is the token every program presents; it does not
//!   start without one.
//!
//! It logs to its standard error. `JUSH_RELEASE` names the release whose
//! command packages the cores' runners run, as the server's does, in one
//! root or a list of them (`crates/core-api/src/process/packages.rs`);
//! without it, they run none.

use std::process::ExitCode;

/// How the process runs.
enum Mode {
    /// Over standard input and output.
    Stdio,
    /// On its own, listening on the address.
    Serve { listen: String },
}

fn main() -> ExitCode {
    // A panic leaves its report on standard error.
    jush_shared_cli::install_panic_hook();
    // Standard output may carry the program's connection, and nothing else.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .init();
    let mode = match mode(std::env::args().skip(1).collect()) {
        Ok(mode) => mode,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(event = "process.runtime_failed", %error, "the runtime did not start");
            return ExitCode::FAILURE;
        }
    };
    let release = std::env::var_os("JUSH_RELEASE")
        .map(|roots| std::env::split_paths(&roots).collect())
        .unwrap_or_default();
    let ran = match mode {
        Mode::Stdio => {
            let connection = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
            runtime.block_on(jush_core_api::process::run(connection, release))
        }
        Mode::Serve { listen } => {
            let token = match std::env::var("JUSH_CORE_TOKEN") {
                Ok(token) if !token.is_empty() => token,
                _ => {
                    tracing::error!(event = "process.token_missing", "JUSH_CORE_TOKEN names no token, which every program presents");
                    return ExitCode::FAILURE;
                }
            };
            let listener = match runtime.block_on(tokio::net::TcpListener::bind(&listen)) {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::error!(event = "process.listen_failed", address = listen, %error, "the process cannot listen on its address");
                    return ExitCode::FAILURE;
                }
            };
            runtime.block_on(jush_core_api::process::serve(listener, token, release, stopped()))
        }
    };
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(
                event = "process.failed",
                error = &error as &dyn std::error::Error,
                "the process did not run"
            );
            ExitCode::FAILURE
        }
    }
}

/// The mode `arguments` name; the usage when they name none.
fn mode(arguments: Vec<String>) -> Result<Mode, &'static str> {
    const USAGE: &str = "usage: jush-core [serve --listen <address>]";
    match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => Ok(Mode::Stdio),
        ["serve", "--listen", listen] => Ok(Mode::Serve {
            listen: (*listen).to_owned(),
        }),
        _ => Err(USAGE),
    }
}

/// Resolves once the process is asked to stop: `SIGTERM`, or Ctrl-C.
async fn stopped() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = terminate.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
                return;
            }
            Err(error) => {
                tracing::warn!(event = "process.signal_unwatched", %error, "SIGTERM does not stop the process; Ctrl-C does");
            }
        }
    }
    // A Ctrl-C that cannot be watched stops nothing; the process then runs
    // until it is killed.
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

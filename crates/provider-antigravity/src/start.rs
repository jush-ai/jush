//! How an `agy` process starts on the Subscriptions machine.
//!
//! - A sign-in runs `agy acp --login` with its new directory as its `HOME`,
//!   where `agy` writes the sign-in (`login.rs`).
//! - Every other process runs `agy acp` with its own directory as its `HOME`
//!   and its working directory, so everything `agy` keeps of the process goes
//!   with the directory. The directory's sign-in is a link to the account's,
//!   which `agy` renews through the link, writing the account's file: every
//!   process of the account shares one sign-in.
//! - No API key in the machine's environment reaches `agy`, so the account
//!   pays, never a key.
//! - `agy` leads a process group of its own, so that what it starts ends with
//!   it.
//!
//! Every flag is one `agy acp --help` lists.

use std::collections::BTreeMap;

use jush_device_interface::{SpawnEnv, SpawnRequest};
use jush_provider_subscription_cli::{CliSite, CliStart};

use crate::CLI;
use crate::account::SIGN_IN;

/// The request that starts `agy`'s own sign-in at `site`, in its new
/// directory. It prints the link to sign in at and reads the code Google's
/// page gives from its input.
pub(crate) fn login(site: &CliSite) -> CliStart {
    let spawn = SpawnRequest {
        command: site.executable.clone(),
        args: ["acp", "--login", "--disable-telemetry"].map(String::from).to_vec(),
        cwd: Some(site.config_dir.clone()),
        env: SpawnEnv::Overlay(environment(&site.config_dir)),
        descriptors: Vec::new(),
        retained: false,
        process_group: true,
    };
    CliStart {
        spawn,
        files: Vec::new(),
        links: Vec::new(),
    }
}

/// The request that starts a probe's `agy acp` at `site`, which is work of
/// the machine while it runs.
pub(crate) fn probe(site: &CliSite) -> CliStart {
    acp(site, false)
}

/// The directory of the process `site` starts: its `HOME`.
pub(crate) fn home(site: &CliSite) -> String {
    format!("{}/{}/{}", site.config_dir, CLI.process_parent, site.process_dir)
}

/// `agy acp` at `site`, with the account's sign-in linked into its
/// directory; `retained` keeps it between pieces of work.
fn acp(site: &CliSite, retained: bool) -> CliStart {
    let home = home(site);
    let spawn = SpawnRequest {
        command: site.executable.clone(),
        args: ["acp", "--disable-telemetry"].map(String::from).to_vec(),
        cwd: Some(home.clone()),
        env: SpawnEnv::Overlay(environment(&home)),
        descriptors: Vec::new(),
        retained,
        process_group: true,
    };
    CliStart {
        spawn,
        files: Vec::new(),
        links: vec![(SIGN_IN.to_owned(), format!("{}/{SIGN_IN}", site.config_dir))],
    }
}

/// The environment of a process whose `HOME` is `home`.
fn environment(home: &str) -> BTreeMap<String, Option<String>> {
    BTreeMap::from([
        ("HOME".to_owned(), Some(home.to_owned())),
        ("GEMINI_API_KEY".to_owned(), None),
        ("GOOGLE_API_KEY".to_owned(), None),
    ])
}

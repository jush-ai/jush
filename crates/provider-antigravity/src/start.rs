//! How an `agy` process starts on the Subscriptions machine.
//!
//! - A sign-in runs `agy acp --login` with its new directory as its `HOME`,
//!   where `agy` writes the sign-in (`login.rs`).
//! - Every other process runs `agy` with its own directory as its `HOME`
//!   and its working directory, so everything `agy` keeps of the process goes
//!   with the directory. The directory's sign-in is a link to the account's,
//!   which `agy` renews through the link, writing the account's file: every
//!   process of the account shares one sign-in.
//! - A session's processes run Jush's agent, a custom agent whose prompt is
//!   the request's system prompt, whose one MCP server is Jush's
//!   (`tools.rs`) and whose one tool of `agy`'s reads back what `agy` saved
//!   of a long tool result; the process that makes a session for its history
//!   to be written into has the agent and no server (`history.rs`).
//! - `agy` asks before every call it does not allow by itself, and Jush
//!   allows only its own tools' (`live.rs`): the machine holds every
//!   account's sign-in, which no model may read. The session's workspace,
//!   whose files `agy` reads without asking, holds only the media Jush gives
//!   the model as files (`media.rs`), none of the process's own.
//! - A usage read runs `agy --print /usage`, which prints the account's usage
//!   and ends (`quota.rs`).
//! - No API key in the machine's environment reaches `agy`, so the account
//!   pays, never a key, and no `agy` updates itself: print mode replaces its
//!   own executable with Google's newest release otherwise (measured), and
//!   that executable is the copy Jush installed and verified
//!   (`releases.rs`). `AGY_CLI_DISABLE_AUTO_UPDATE` is the switch Google's
//!   troubleshooting guide documents.
//! - `agy` leads a process group of its own, so that what it starts ends with
//!   it.
//!
//! Every flag is one `agy acp --help` or `agy --help` lists.

use std::collections::BTreeMap;

use bytes::Bytes;
use jush_device_interface::{SpawnEnv, SpawnRequest};
use jush_provider_subscription_cli::{CliSite, CliStart};

use crate::CLI;
use crate::account::SIGN_IN;
use crate::tools;

/// The name of Jush's agent, and of its file in the process's directory, its
/// working directory, where `agy` finds the agent it starts with.
const AGENT: &str = "jush";
const AGENT_FILE: &str = ".agents/agents/jush.md";

/// Where `agy` keeps its sessions' databases in its `HOME`.
pub(crate) const CONVERSATIONS: &str = ".gemini/antigravity-cli/conversations";
/// What the agent's prompt says of its one tool of `agy`'s, after the
/// request's system prompt: without it the model reads the user's files with
/// it, which Jush denies (measured).
const READ_FILE: &str = "Your read_file tool reads only the files in your workspace and the files Antigravity saves on its own machine when a tool result is too long for it; read anything else with shell.";

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
        companion: None,
        files: Vec::new(),
        links: Vec::new(),
    }
}

/// The request that starts a probe's `agy acp` at `site`, which is work of
/// the machine while it runs.
pub(crate) fn probe(site: &CliSite) -> CliStart {
    acp(site, Vec::new(), false)
}

/// The request that starts `agy --print /usage` at `site`, which prints the
/// account's usage as JSON and ends; it makes no model request. It has no
/// `--disable-telemetry`, which print mode rejects as an unknown flag.
pub(crate) fn usage(site: &CliSite) -> CliStart {
    let args = ["--print", "/usage", "--output-format", "json"].map(String::from).to_vec();
    process(site, args, false)
}

/// The request that starts a process at `site` that makes a session of Jush's
/// agent, with `system_prompt` as its prompt, for its history to be written
/// into: it has no MCP server, since it makes no model request.
pub(crate) fn maker(site: &CliSite, system_prompt: &str) -> CliStart {
    let mut start = acp(site, agent_args(), false);
    start.files.push((AGENT_FILE.to_owned(), agent(system_prompt)));
    start
}

/// The request that starts a session's process at `site`, kept between its
/// turns, with Jush's agent and its tools; `session` is a session's database
/// with its history, by its id, which the process loads.
pub(crate) fn turn(site: &CliSite, system_prompt: &str, session: Option<(&str, Bytes)>) -> CliStart {
    let home = home(site);
    let mut start = acp(site, agent_args(), true);
    start.files.push((AGENT_FILE.to_owned(), agent(system_prompt)));
    start.files.push((tools::CONFIG.to_owned(), tools::config(&home)));
    if let Some((id, database)) = session {
        start.files.push((format!("{CONVERSATIONS}/{id}.db"), database));
    }
    start.companion = Some(tools::companion(&home));
    start
}

/// The directory of the process `site` starts: its `HOME`.
pub(crate) fn home(site: &CliSite) -> String {
    format!("{}/{}/{}", site.config_dir, CLI.process_parent, site.process_dir)
}

/// The workspace of the session the process `site` starts opens.
pub(crate) fn workspace(site: &CliSite) -> String {
    format!("{}/workspace", home(site))
}

/// `agy acp` at `site` with `args`, and the account's sign-in linked into its
/// directory; `retained` keeps it between pieces of work.
fn acp(site: &CliSite, args: Vec<String>, retained: bool) -> CliStart {
    let args = ["acp", "--disable-telemetry"].map(String::from).into_iter().chain(args).collect();
    process(site, args, retained)
}

/// `agy` at `site` with `args`, and the account's sign-in linked into its
/// directory; `retained` keeps it between pieces of work.
fn process(site: &CliSite, args: Vec<String>, retained: bool) -> CliStart {
    let home = home(site);
    let spawn = SpawnRequest {
        command: site.executable.clone(),
        args,
        cwd: Some(home.clone()),
        env: SpawnEnv::Overlay(environment(&home)),
        descriptors: Vec::new(),
        retained,
        process_group: true,
    };
    CliStart {
        spawn,
        companion: None,
        files: Vec::new(),
        links: vec![(SIGN_IN.to_owned(), format!("{}/{SIGN_IN}", site.config_dir))],
    }
}

/// The arguments that run Jush's agent.
fn agent_args() -> Vec<String> {
    ["--agent", AGENT].map(String::from).to_vec()
}

/// Jush's agent: `agy`'s `view_file`, which the model calls `read_file`,
/// Jush's MCP server, never a subagent of another, and `system_prompt` as its
/// prompt, with what it says of `read_file`.
///
/// `agy` gives the model a tool's result up to 10,240 bytes, and saves a
/// larger image or the whole of a longer text to a file of its own;
/// `read_file` is the only way the model reads it.
///
/// `excludeDefaultComponents` leaves out the parts `agy` adds to every
/// agent's prompt, about 2,100 tokens a request (measured): its workspace,
/// artifact, messaging and link-style instructions, which name `agy`'s own
/// directories and ways and contradict Jush's prompt.
fn agent(system_prompt: &str) -> Bytes {
    let header = format!(
        "---\nname: {AGENT}\ndescription: Jush's agent.\ntools: [view_file]\nmcpServers:\n  - name: {}\nmainAgent: true\nsubagent: false\nexcludeDefaultComponents: true\n---\n",
        tools::SERVER
    );
    Bytes::from(format!("{header}{system_prompt}\n\n{READ_FILE}\n"))
}

/// The environment of a process whose `HOME` is `home`.
fn environment(home: &str) -> BTreeMap<String, Option<String>> {
    BTreeMap::from([
        ("HOME".to_owned(), Some(home.to_owned())),
        ("GEMINI_API_KEY".to_owned(), None),
        ("GOOGLE_API_KEY".to_owned(), None),
        ("AGY_CLI_DISABLE_AUTO_UPDATE".to_owned(), Some("true".to_owned())),
    ])
}

//! How an `agy` process starts on the Subscriptions machine.
//!
//! - A sign-in runs `agy acp --login` with its new directory as its `HOME`,
//!   where `agy` writes the sign-in (`login.rs`).
//! - Every other process runs `agy acp` with its own directory as its `HOME`
//!   and its working directory, so everything `agy` keeps of the process goes
//!   with the directory. The directory's sign-in is a link to the account's,
//!   which `agy` renews through the link, writing the account's file: every
//!   process of the account shares one sign-in.
//! - A session's processes run Jush's agent, a custom agent whose prompt is
//!   the request's system prompt and whose one MCP server is Jush's
//!   (`tools.rs`); the process that makes a session for its history to be
//!   written into has the agent and no server (`history.rs`).
//! - No API key in the machine's environment reaches `agy`, so the account
//!   pays, never a key.
//! - `agy` leads a process group of its own, so that what it starts ends with
//!   it.
//!
//! Every flag is one `agy acp --help` lists.

use std::collections::BTreeMap;

use bytes::Bytes;
use jush_device_interface::{SpawnEnv, SpawnRequest};
use jush_provider_subscription_cli::{CliSite, CliStart};

use crate::CLI;
use crate::account::SIGN_IN;
use crate::tools;

/// The name of Jush's agent, and of its file in the process's directory,
/// where `agy` finds a workspace's custom agents.
const AGENT: &str = "jush";
const AGENT_FILE: &str = ".agents/agents/jush.md";

/// Where `agy` keeps its sessions' databases in its `HOME`.
pub(crate) const CONVERSATIONS: &str = ".gemini/antigravity-cli/conversations";

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

/// `agy acp` at `site` with `args`, and the account's sign-in linked into its
/// directory; `retained` keeps it between pieces of work.
fn acp(site: &CliSite, args: Vec<String>, retained: bool) -> CliStart {
    let home = home(site);
    let spawn = SpawnRequest {
        command: site.executable.clone(),
        args: ["acp", "--disable-telemetry"]
            .map(String::from)
            .into_iter()
            .chain(args)
            .collect(),
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

/// The arguments that run Jush's agent with no question asked before a tool
/// call: the model's calls are Jush's to run.
fn agent_args() -> Vec<String> {
    ["--agent", AGENT, "--dangerously-skip-permissions"]
        .map(String::from)
        .to_vec()
}

/// Jush's agent: none of `agy`'s own tools, Jush's MCP server, never a
/// subagent of another, and `system_prompt` as its prompt.
fn agent(system_prompt: &str) -> Bytes {
    let header = format!(
        "---\nname: {AGENT}\ndescription: Jush's agent.\ntools: []\nmcpServers:\n  - name: {}\nmainAgent: true\nsubagent: false\n---\n",
        tools::SERVER
    );
    Bytes::from(header + system_prompt)
}

/// The environment of a process whose `HOME` is `home`.
fn environment(home: &str) -> BTreeMap<String, Option<String>> {
    BTreeMap::from([
        ("HOME".to_owned(), Some(home.to_owned())),
        ("GEMINI_API_KEY".to_owned(), None),
        ("GOOGLE_API_KEY".to_owned(), None),
    ])
}

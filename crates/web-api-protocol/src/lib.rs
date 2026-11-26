//! The web app's REST contract: every request and response body the backend
//! serves, the error body with the one list of its codes, and the identifier
//! and text types those bodies use.
//!
//! The backend decodes each request into its type and serializes each response
//! from its type. The data the web app, the agent and the backend share, such
//! as times, comes from `core`.
//!
//! - A type the backend receives refuses unknown fields. A type only the web
//!   app receives accepts them (`crates/xtask/src/contracts/mod.rs`).
//! - A response carries the fields its type declares and nothing more: what the
//!   backend keeps for itself in a stored record, such as a conversation's
//!   owner or its message counts, never reaches the web app.
//! - The crate reuses the runner's working-tree change types and platform, and
//!   the command locale, rather than declaring them again.
//! - A device log line is its own type: the runner wire carries its time as
//!   integer milliseconds and the web app receives an RFC 3339 time.
//!
//! The crate holds no route handling and no domain logic.

pub mod attachments;
pub mod auth;
pub mod schedules;
pub mod cloud;
pub mod command_history;
pub mod connection;
pub mod conversations;
pub mod cores;
pub mod credentials;
pub mod debug;
pub mod devices;
pub mod diagnostics;
pub mod drafts;
pub mod error;
pub mod files;
pub mod attached_devices;
pub mod pins;
pub mod ids;
pub mod instructions;
pub mod work;
pub mod page;
pub mod passkeys;
pub mod permissions;
pub mod plugins;
pub mod port_forwards;
pub mod providers;
pub mod push;
pub mod query;
pub mod saved;
pub mod search;
pub mod server_upgrade;
pub mod sessions;
pub mod settings;
pub mod shares;
pub mod side_questions;
pub mod sidebar;
pub mod starter;
pub mod state;
pub mod storage;
pub mod subagents;
pub mod text;
pub mod usage;
pub mod users;
pub mod workflows;
pub mod projects;
pub mod workspaces;

/// The largest message a page sends on a lane other than a program's
/// plugin's: a conversation's, the synchronization channel's and a user
/// stream's. A frame
/// refers to an upload and never carries its bytes, and a user stream frames
/// its own messages, so no page needs a larger one.
///
/// Bounded because a message is read whole into memory and a client sends
/// without end.
pub const MAX_PAGE_MESSAGE_BYTES: usize = 1024 * 1024;

/// The largest message a program sends on its plugin's lane: a reply or a
/// port message carries its bytes whole, as the plugin contract passes
/// them, and the largest a plugin passes is a video or a fetched file of 64
/// MiB.
///
/// Bounded because a message is read whole into memory and a client sends
/// without end.
pub const MAX_PLUGIN_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// A command's path as a conversation's list names it: command names, a
/// single space between two, such as `conversation list`, without a first
/// `jush`.
fn command_path(text: &str, _: &()) -> garde::Result {
    let mut words = text.split(' ');
    if words.clone().all(jush_command_protocol::is_command_name) && words.next() != Some("jush") {
        Ok(())
    } else {
        Err(garde::Error::new(
            "is not a command's path: command names, a single space between two, without `jush`",
        ))
    }
}

/// Whether `text` names a version of a program's plugin: 1 to 64 letters,
/// digits, `.`, `_` and `-`, which a query and a lane's open carry as they
/// are.
pub fn is_plugin_version(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// A conversation's versions of the program's plugins: each a version by
/// its plugin's id.
fn plugin_versions(versions: &std::collections::BTreeMap<String, String>, _: &()) -> garde::Result {
    for (plugin, version) in versions {
        if plugin.is_empty() {
            return Err(garde::Error::new("a version names no plugin"));
        }
        if !is_plugin_version(version) {
            return Err(garde::Error::new(format!(
                "{plugin}: {version} is no plugin's version: 1 to 64 letters, digits, `.`, `_` and `-`"
            )));
        }
    }
    Ok(())
}

/// A check of a text that must say something: whitespace alone says
/// nothing.
fn not_blank(text: &str, _: &()) -> garde::Result {
    if jush_shared_types::is_blank(text) {
        Err(garde::Error::new("is blank"))
    } else {
        Ok(())
    }
}

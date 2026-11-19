//! The starting points `find` refuses to walk to any depth: a disk's root,
//! the home folder of the user the runner runs as, and the folder that
//! holds the home folders. `find` itself reads the starting points from its
//! command line and asks, unless `-maxdepth` bounds the walk
//! (`findutils::find::Dependencies::refuse_start`).
//!
//! Walking one of them takes minutes, fills the call's output, and on a Mac
//! can make the system ask the user for access to each private folder it
//! enters; a file the agent looks for is in a folder it can name. The
//! refusal says what to run instead, so it does not read as "not found".

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

/// Why `find` must not walk `start` to any depth, or `None` when it may.
pub(super) fn refusal(start: &OsStr) -> Option<String> {
    let resolved = std::fs::canonicalize(uucore::context::resolve(start)).ok()?;
    let what = kind(&resolved)?;
    Some(message(start, what))
}

/// What `path`, resolved, is when `find` refuses it.
fn kind(path: &Path) -> Option<&'static str> {
    if path.parent().is_none() || data_volume().is_some_and(|volume| same(path, &volume)) {
        return Some("the root of a disk");
    }
    let home = std::env::home_dir().and_then(|home| std::fs::canonicalize(home).ok())?;
    if same(path, &home) {
        return Some("a home folder");
    }
    // Only the system's own folder of home folders: a home elsewhere, such
    // as a service's under /var/lib, has an ordinary folder above it.
    let homes = home
        .parent()
        .filter(|parent| parent.file_name().is_some_and(|name| ["Users", "home"].iter().any(|homes| name.eq_ignore_ascii_case(homes))));
    homes
        .is_some_and(|homes| same(path, homes))
        .then_some("the folder that holds the home folders")
}

/// The root of a Mac's data volume, which holds what `/` shows of the
/// user's files under another path: `/System/Volumes/Data/Users/alex` is
/// the home folder `/Users/alex`.
fn data_volume() -> Option<PathBuf> {
    cfg!(target_os = "macos").then(|| PathBuf::from("/System/Volumes/Data"))
}

/// Whether `a` and `b` are one folder: on a Mac a folder of the data
/// volume is the same folder as the one `/` shows, by another path.
#[cfg(unix)]
fn same(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => a == b,
    }
}

#[cfg(windows)]
fn same(a: &Path, b: &Path) -> bool {
    a == b
}

fn message(start: &OsStr, what: &str) -> String {
    let start = start.to_string_lossy();
    let cost = if cfg!(target_os = "macos") {
        "walking all of it takes minutes and can make macOS ask the user for access to their private folders"
    } else {
        "walking all of it takes minutes and floods the output"
    };
    let mut text = format!(
        "find: '{start}' is {what}: {cost}. Search the folder the file belongs in, or bound the depth with -maxdepth."
    );
    if cfg!(target_os = "macos") {
        text.push_str(" mdfind finds a file by name from Spotlight's index at once.");
    } else if cfg!(target_os = "linux") && !crate::which::locate(OsStr::new("locate"), false).is_empty() {
        text.push_str(" locate finds a file by name from its index at once.");
    }
    text
}

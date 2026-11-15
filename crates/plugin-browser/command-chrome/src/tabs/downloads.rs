//! Every download of a conversation's browsers on this device: where it
//! goes, how far it is, and which tab started it.
//!
//! ```text
//! Chrome --downloadWillBegin--> Downloads --> its list (watch)
//!        --downloadProgress-->      |          |-> the live view's bubble
//!                                   |          |-> `download`'s list
//!   <profile>/downloads/<guid>      |          '-> the agent's next command,
//!   while in progress               v              which tells the new ones
//!   <scratch>/downloads/<name>, numbered `report (1).csv`, once complete
//! ```
//!
//! - Chrome writes a download in progress under its id in the profile's
//!   download directory, so a partial file never carries a name; the file
//!   moves to the folder once complete. The folder is the conversation's,
//!   and so is this list: both outlast every browser.
//! - A download's name is taken as it begins, numbered past the folder's
//!   files and the downloads in progress, so the name told at once is the
//!   file's. Only a file that appears in the folder meanwhile moves it on.
//! - The agent is told of every download, whoever started it: the user and
//!   the agent share the browser, and a download a page starts later, in a
//!   frame or a new tab, has no command to belong to.
//! - A suggested file name is data, never permission for path traversal.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chromiumoxide::cdp::browser_protocol::{
    browser::{DownloadProgressState, EventDownloadProgress, EventDownloadWillBegin},
    page::FrameId,
};
use futures_util::{FutureExt as _, StreamExt};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::driver::operation::Result;
use crate::tabs::{environment::BrowserHandle, protocol::TabId, registry::Tabs, tab::BrowserTab};
use crate::traced::SpawnTraced as _;
use jush_command_package_browser_protocol::live::DownloadState;

/// The longest name, in bytes, its number included: file systems take 255
/// bytes, and a number adds at most ` (999)`.
const NAME_BYTES: usize = 255 - " (999)".len();
/// A name taken this many times over gives up on a numbered one.
const NUMBERED: u32 = 1000;

/// One download of the conversation's browsers.
#[derive(Clone, Debug, PartialEq)]
pub struct Download {
    /// Chrome's identity of the download, the name of its partial file.
    pub id: String,
    /// The tab whose page started it; none until it is found, or when no
    /// tab holds the frame.
    pub tab: Option<TabId>,
    /// The file's name in the folder, the page's numbered when taken.
    pub name: String,
    pub state: DownloadState,
    /// The bytes received so far.
    pub received: u64,
    /// The file's size; 0 while the server has not said it.
    pub total: u64,
    /// The file, once complete.
    pub path: Option<PathBuf>,
    pub began: SystemTime,
    /// The name the page suggested, which a taken name numbers.
    suggested: String,
    /// The agent has been told of it.
    told: bool,
}

/// The downloads of one conversation on this device.
#[derive(Clone)]
pub struct Downloads {
    list: watch::Sender<Vec<Download>>,
    folder: PathBuf,
}

impl Downloads {
    /// The conversation's downloads, whose complete files go into `folder`.
    pub fn new(folder: PathBuf) -> Self {
        Self {
            list: watch::channel(Vec::new()).0,
            folder,
        }
    }

    /// Follows the downloads of the browser `handle` reaches, which Chrome
    /// writes into `partial`, until `ended`; then those in progress end
    /// canceled, since Chrome and its partial files are gone.
    pub(crate) async fn follow(
        &self,
        handle: &BrowserHandle,
        tabs: Tabs,
        partial: PathBuf,
        tasks: &TaskTracker,
        ended: CancellationToken,
    ) -> Result<()> {
        let browser = handle.call()?;
        let mut beginnings = browser.event_listener::<EventDownloadWillBegin>().await?;
        let mut progress = browser.event_listener::<EventDownloadProgress>().await?;
        drop(browser);
        let follower = self.clone();
        let finders = tasks.clone();
        tasks.spawn_traced(async move {
            loop {
                tokio::select! {
                    // Chrome tells a download's beginning before its progress.
                    biased;
                    beginning = beginnings.next() => match beginning {
                        Some(Ok(beginning)) => follower.begin(&tabs, &beginning, &finders).await,
                        // A lagged listener lost a beginning; that download goes unlisted.
                        Some(Err(_)) => {}
                        None => break,
                    },
                    event = progress.next() => match event {
                        Some(Ok(event)) => follower.advance(&partial, &event).await,
                        // A lagged listener lost a progress; the next one says where it stands.
                        Some(Err(_)) => {}
                        None => break,
                    },
                    _ = ended.cancelled() => break,
                }
            }
            // A download Chrome completed before it ended is named: its file
            // stays until the retirement empties the directory.
            while let Some(Some(Ok(event))) = progress.next().now_or_never() {
                follower.advance(&partial, &event).await;
            }
            // One browser runs at a time, so every download in progress was
            // this one's.
            follower.list.send_if_modified(|list| {
                let mut ended = false;
                for download in list.iter_mut().filter(|download| download.state == DownloadState::InProgress) {
                    download.state = DownloadState::Canceled;
                    ended = true;
                }
                ended
            });
        });
        Ok(())
    }

    /// The folder complete downloads are in.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Every download, the oldest first, as it changes.
    pub fn subscribe(&self) -> watch::Receiver<Vec<Download>> {
        self.list.subscribe()
    }

    /// Every download now, the oldest first.
    pub fn list(&self) -> Vec<Download> {
        self.list.borrow().clone()
    }

    /// The downloads the agent has not been told of, which the caller tells
    /// now.
    pub fn take_untold(&self) -> Vec<Download> {
        let mut taken = Vec::new();
        self.list.send_if_modified(|list| {
            for download in list.iter_mut().filter(|download| !download.told) {
                download.told = true;
                taken.push(download.clone());
            }
            !taken.is_empty()
        });
        taken
    }

    /// Waits until no download is in progress, or `deadline`.
    pub async fn settled(&self, deadline: tokio::time::Instant) {
        let mut list = self.list.subscribe();
        let _ended = tokio::time::timeout_at(
            deadline,
            list.wait_for(|list| list.iter().all(|download| download.state != DownloadState::InProgress)),
        )
        .await;
    }

    /// Lists a download that begins, under a name no file and no other
    /// download has. A main frame's id is its tab's, which finds most tabs at
    /// once; a frame's tab is found apart, so the list never waits on a tab.
    async fn begin(&self, tabs: &Tabs, beginning: &EventDownloadWillBegin, finders: &TaskTracker) {
        let suggested = file_name(&beginning.suggested_filename);
        let mut name = suggested.clone();
        for number in 1..NUMBERED {
            let listed = self
                .list
                .borrow()
                .iter()
                .any(|download| download.state == DownloadState::InProgress && download.name == name);
            if !listed && !tokio::fs::try_exists(self.folder.join(&name)).await.unwrap_or(true) {
                break;
            }
            name = numbered(&suggested, number);
        }
        let listed = tabs.latest();
        let tab = listed
            .tabs
            .iter()
            .find(|listed| listed.tab.page().target_id().as_ref() == beginning.frame_id.as_ref())
            .map(|listed| listed.tab.id().clone());
        let found = tab.is_some();
        self.list.send_modify(|list| {
            list.push(Download {
                id: beginning.guid.clone(),
                tab,
                name,
                state: DownloadState::InProgress,
                received: 0,
                total: 0,
                path: None,
                began: SystemTime::now(),
                suggested,
                told: false,
            });
        });
        if found {
            return;
        }
        let downloads = self.list.clone();
        let tabs = tabs.clone();
        let frame = beginning.frame_id.clone();
        let id = beginning.guid.clone();
        finders.spawn_traced(async move {
            let Some(tab) = owner(&tabs, &frame).await else {
                return;
            };
            downloads.send_if_modified(|list| match list.iter_mut().find(|download| download.id == id) {
                Some(download) => {
                    download.tab = Some(tab.id().clone());
                    true
                }
                None => false,
            });
        });
    }

    async fn advance(&self, partial: &Path, event: &EventDownloadProgress) {
        let Some((name, suggested)) = self
            .list
            .borrow()
            .iter()
            .find(|download| download.id == event.guid && download.state == DownloadState::InProgress)
            .map(|download| (download.name.clone(), download.suggested.clone()))
        else {
            return;
        };
        let received = event.received_bytes.max(0.0) as u64;
        let total = event.total_bytes.max(0.0) as u64;
        let (state, path) = match event.state {
            DownloadProgressState::InProgress => (DownloadState::InProgress, None),
            DownloadProgressState::Canceled => (DownloadState::Canceled, None),
            DownloadProgressState::Completed => match name_file(&partial.join(&event.guid), &self.folder, &name, &suggested).await {
                Ok(path) => (DownloadState::Complete, Some(path)),
                Err(error) => {
                    // The file stays under Chrome's id, which the
                    // retirement removes; nothing can offer it.
                    tracing::warn!(event = "browser_download.failed", guid = %event.guid, %error, "a download could not be named");
                    (DownloadState::Canceled, None)
                }
            },
        };
        self.list.send_if_modified(|list| {
            let Some(download) = list.iter_mut().find(|download| download.id == event.guid) else {
                return false;
            };
            let next = Download {
                state,
                received,
                // A complete file's size is what came, when the server never said it.
                total: if total == 0 && state == DownloadState::Complete { received } else { total },
                name: path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map_or(download.name.clone(), |name| name.to_string_lossy().into_owned()),
                path: path.clone(),
                ..download.clone()
            };
            if *download == next {
                return false;
            }
            *download = next;
            true
        });
    }
}

/// The tab whose page holds `frame`, cross-site frames included. A main
/// frame's id is its target's, which finds most downloads without asking
/// Chrome.
async fn owner(tabs: &Tabs, frame: &FrameId) -> Option<BrowserTab> {
    let listed = tabs.latest();
    if let Some(found) = listed.tabs.iter().find(|listed| listed.tab.page().target_id().as_ref() == frame.as_ref()) {
        return Some(found.tab.clone());
    }
    for listed in &listed.tabs {
        if let Ok(snapshot) = crate::driver::frames::capture(listed.tab.page()).await
            && snapshot.frames.iter().any(|document| &document.frame.id == frame)
        {
            return Some(listed.tab.clone());
        }
    }
    None
}

/// Moves `source` to `name` in `folder`, or when a file took it meanwhile,
/// to `suggested` numbered as a browser numbers a name already taken:
/// `report (1).pdf`. A name is taken by linking, which never replaces a file.
async fn name_file(source: &Path, folder: &Path, name: &str, suggested: &str) -> std::io::Result<PathBuf> {
    tokio::fs::create_dir_all(folder).await?;
    let names = std::iter::once(name.to_owned()).chain((1..NUMBERED).map(|number| numbered(suggested, number)));
    for candidate in names {
        let candidate = folder.join(candidate);
        match tokio::fs::hard_link(source, &candidate).await {
            Ok(()) => {
                tokio::fs::remove_file(source).await?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other(format!("every name for {suggested} is taken")))
}

/// `name` with `number`, as a browser numbers a name already taken:
/// `report (1).pdf`.
fn numbered(name: &str, number: u32) -> String {
    let (stem, extension) = split(name);
    format!("{stem} ({number}){extension}")
}

/// A name's stem and its extension, with its dot.
fn split(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    }
}

/// The file name a page suggested, as data: its last component, without
/// control characters, never empty, `.` or `..`, and short enough to number,
/// its stem shortened.
fn file_name(suggested: &str) -> String {
    let last = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last.chars().filter(|character| !character.is_control()).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return "download".to_owned();
    }
    let (stem, extension) = split(trimmed);
    // An extension too long to keep is part of the stem.
    let (stem, extension) = if extension.len() > NAME_BYTES / 2 { (trimmed, "") } else { (stem, extension) };
    let mut end = (NAME_BYTES - extension.len()).min(stem.len());
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{extension}", stem[..end].trim_end())
}

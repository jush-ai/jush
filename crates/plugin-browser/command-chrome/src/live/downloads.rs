//! The downloads that start in a watched tab, as the page's download bubble
//! shows them: the conversation's downloads (`crate::tabs::downloads`) of
//! this tab since the tab is watched, the agent's and the user's alike, as a
//! browser's bubble shows every download of its window.

use std::collections::HashSet;

use tokio::sync::watch;
use tokio_util::task::TaskTracker;

use crate::driver::operation::Result;
use crate::tabs::{downloads::Downloads, tab::BrowserTab};
use crate::traced::SpawnTraced as _;
use jush_command_package_browser_protocol::live::{LiveDownload, MAX_DOWNLOADS};

/// Follows the downloads that start in `tab` from now until it ends, into
/// `bubble`: one that started before opens no bubble now.
pub(crate) async fn follow(
    tab: &BrowserTab,
    downloads: &Downloads,
    bubble: watch::Sender<Vec<LiveDownload>>,
    tasks: &TaskTracker,
) -> Result<()> {
    let mut list = downloads.subscribe();
    let earlier: HashSet<String> = list.borrow_and_update().iter().map(|download| download.id.clone()).collect();
    let tab = tab.clone();
    tasks.spawn_traced(async move {
        loop {
            let shown: Vec<LiveDownload> = {
                let list = list.borrow_and_update();
                let mine: Vec<_> = list
                    .iter()
                    .filter(|download| download.tab.as_ref() == Some(tab.id()) && !earlier.contains(&download.id))
                    .collect();
                let excess = mine.len().saturating_sub(MAX_DOWNLOADS);
                mine[excess..]
                    .iter()
                    .map(|download| LiveDownload {
                        id: download.id.clone(),
                        name: download.name.clone(),
                        state: download.state,
                        received: download.received,
                        total: download.total,
                        path: download
                            .path
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    })
                    .collect()
            };
            bubble.send_if_modified(|current| {
                if *current == shown {
                    return false;
                }
                *current = shown;
                true
            });
            tokio::select! {
                _ = tab.ended().cancelled() => break,
                changed = list.changed() => if changed.is_err() { break },
            }
        }
    });
    Ok(())
}

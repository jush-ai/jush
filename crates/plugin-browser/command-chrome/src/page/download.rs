//! `download`: the conversation's downloads on this device, the newest first.
//!
//! The conversation's downloads say how far each is; the files in the
//! downloads folder are the complete ones, those a resident before this one
//! named included. A complete download whose file has gone is no longer
//! listed. No browser starts for it.

use std::time::SystemTime;

use serde_json::Value;

use crate::driver::operation::Result;
use crate::page::protocol::{BrowserDownload, DownloadResult};
use crate::tabs::downloads::Downloads;
use jush_command_package_browser_protocol::live::DownloadState;

/// What `wait` leaves of the deadline to list: the list itself is quick.
const LISTING: std::time::Duration = std::time::Duration::from_secs(1);

/// The conversation's `downloads`; with `wait`, once those in progress have
/// ended or the deadline is near.
pub async fn list(downloads: &Downloads, wait: bool, deadline: tokio::time::Instant) -> Result<Value> {
    if wait {
        downloads.settled(deadline - LISTING).await;
    }
    let folder = downloads.folder();
    // The folder is read before the list, so a download that completes
    // between them is in the list alone, never in both.
    let mut rows: Vec<(SystemTime, BrowserDownload)> = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(folder).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            // A hidden file is the system's, such as Finder's `.DS_Store`.
            if name.starts_with('.') {
                continue;
            }
            let Ok(metadata) = entry.metadata().await else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            rows.push((
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                BrowserDownload {
                    name,
                    state: DownloadState::Complete,
                    received: metadata.len(),
                    total: metadata.len(),
                    path: entry.path().to_string_lossy().into_owned(),
                },
            ));
        }
    }
    for download in downloads.list() {
        let path = download
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        if download.state == DownloadState::Complete {
            // The folder's row of the file, or none when it has gone.
            rows.retain(|(_, row)| row.path != path);
            if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
                continue;
            }
        }
        rows.push((
            download.began,
            BrowserDownload {
                name: download.name,
                state: download.state,
                received: download.received,
                total: download.total,
                path,
            },
        ));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    crate::driver::output::value(DownloadResult {
        downloads: rows.into_iter().map(|(_, row)| row).collect(),
        folder: folder.to_string_lossy().into_owned(),
    })
}

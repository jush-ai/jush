//! The media `agy` gives its model only as files: a video, a recording or a
//! PDF, in a prompt or in a tool's result.
//!
//! `agy` takes images alone in a prompt, drops a recording from a tool's
//! result, and saves any other medium of a result as `media_0.png`, whose
//! wrong extension its `read_file` then misreads. Its `read_file` reads a
//! PDF, a recording or a video by the file's extension, and reads files in
//! the session's workspace without asking Jush. So each such medium is
//! written there under its type's extension, and the text in its place
//! names the file, which the model then reads:
//!
//! ```text
//! [audio: /…/processes/<id>/workspace/media/6f1c0a9e2b7d4e58.mp3]
//! ```
//!
//! A file is named by its bytes, so the same bytes are written once. The
//! files go with the process's directory. `read_file` reads no `flac` or
//! `aiff` (measured), which the catalog leaves out (`models.rs`).

use std::collections::HashSet;

use jush_provider_common::{MediaBytes, Medium, ProviderFailure, ResultPart, UserPart};
use jush_provider_subscription_cli::ConfigDir;
use jush_shared_types::{BlobRef, model_media_type_for};

/// The directory of the files, in the process's directory: inside the
/// session's workspace, which `agy` reads without asking.
const MEDIA_DIR: &str = "workspace/media";

/// How many hex digits of a file's SHA-256 name it: enough that two media
/// of one process never share a name.
const NAME_DIGITS: usize = 16;

/// The media a process was given as files, by their names.
#[derive(Default)]
pub(crate) struct Files {
    written: HashSet<String>,
}

impl Files {
    /// `content` as the prompt gives it: each medium `agy` takes only as a
    /// file written into `dir`, the directory of the process at `home`, and
    /// named in text.
    pub(crate) async fn message(
        &mut self,
        dir: &dyn ConfigDir,
        home: &str,
        content: &[UserPart],
    ) -> Result<Vec<UserPart>, ProviderFailure> {
        let mut parts = Vec::with_capacity(content.len());
        for part in content {
            let bytes = match part {
                UserPart::Video(Medium::Bytes(bytes)) | UserPart::Audio(bytes) => bytes,
                UserPart::Document { bytes, .. } => bytes,
                UserPart::Text(_) | UserPart::Image(_) | UserPart::Video(Medium::Url(_)) => {
                    parts.push(part.clone());
                    continue;
                }
            };
            parts.push(UserPart::Text(self.file(dir, home, bytes).await?));
        }
        Ok(parts)
    }

    /// A tool's `output` as its result gives it, the same way.
    pub(crate) async fn result(
        &mut self,
        dir: &dyn ConfigDir,
        home: &str,
        output: &[ResultPart],
    ) -> Result<Vec<ResultPart>, ProviderFailure> {
        let mut parts = Vec::with_capacity(output.len());
        for part in output {
            let bytes = match part {
                ResultPart::Video(bytes) | ResultPart::Audio(bytes) => bytes,
                ResultPart::Document { bytes, .. } => bytes,
                ResultPart::Text(_) | ResultPart::Image(_) => {
                    parts.push(part.clone());
                    continue;
                }
            };
            parts.push(ResultPart::Text(self.file(dir, home, bytes).await?));
        }
        Ok(parts)
    }

    /// Writes `bytes` into the process's workspace, unless it holds them
    /// already: the text that names the file.
    async fn file(
        &mut self,
        dir: &dyn ConfigDir,
        home: &str,
        bytes: &MediaBytes,
    ) -> Result<String, ProviderFailure> {
        let Some(entry) = model_media_type_for(&bytes.media_type) else {
            return Ok(bytes.unsent_text());
        };
        let digest = BlobRef::of(&bytes.data);
        let name = format!(
            "{MEDIA_DIR}/{}.{}",
            &digest.as_str()[..NAME_DIGITS],
            entry.extension
        );
        if !self.written.contains(&name) {
            dir.write(&name, bytes.data.clone().into_bytes())
                .await
                .map_err(|error| {
                    ProviderFailure::no_answer(format!(
                        "Antigravity's workspace could not be written: {error}"
                    ))
                })?;
            self.written.insert(name.clone());
        }
        Ok(format!("[{}: {home}/{name}]", entry.kind.name()))
    }
}

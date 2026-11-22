//! The media a declared command returns (`crates/runner-jobs/src/commands/command_output.rs`):
//! images, videos, audio and PDF documents that `jush file view` hands to its job,
//! their bound, how their bytes are recognized, and the facts a medium
//! record carries of one.

use serde::{Deserialize, Serialize};

/// The largest medium a command returns: a medium is held whole in memory, and
/// a model's vendor accepts no more than a few megabytes of one.
pub const MAX_MEDIUM_BYTES: u64 = 16 * 1024 * 1024;

/// The media type `bytes` begin with, from their magic numbers, or `None` for
/// anything but the image, video, audio and document types a model reads
/// (`crates/shared-types/src/model.rs`).
///
/// - Fewer than 12 bytes are never recognized.
/// - An ISO media file (`ftyp`) is QuickTime for the brand `qt  `, M4V for a
///   brand starting `M4V`, M4A audio for the brand `M4A `, and MP4 otherwise.
/// - An EBML header is WebM, the Matroska format a model reads.
/// - `%PDF-` begins a PDF.
/// - MP3 begins with an ID3v2 tag, its version 2 to 4 and its size four
///   7-bit bytes, or an MPEG Layer III frame header, whose bitrate and
///   sample rate are valid: text that starts `ID3` and a UTF-16 byte order
///   mark, `FF FE`, are neither.
/// - Ogg is audio unless its first packet is Theora video.
pub fn sniff_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }
    let media_type = if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        "video/webm"
    } else if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"qt  " {
            "video/quicktime"
        } else if brand.starts_with(b"M4V") {
            "video/x-m4v"
        } else if brand == b"M4A " {
            "audio/mp4"
        } else {
            "video/mp4"
        }
    } else if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        "audio/wav"
    } else if bytes.starts_with(b"FORM") && (&bytes[8..12] == b"AIFF" || &bytes[8..12] == b"AIFC") {
        "audio/aiff"
    } else if bytes.starts_with(b"OggS") && !contains(&bytes[..bytes.len().min(OGG_HEAD)], b"\x80theora") {
        "audio/ogg"
    } else if bytes.starts_with(b"fLaC") {
        "audio/flac"
    } else if is_id3_tag(bytes) || is_mp3_frame(bytes) {
        "audio/mpeg"
    } else {
        return None;
    };
    Some(media_type)
}

/// How far into an Ogg file its first packet starts: a page header of 27
/// bytes, its segment table, then the packet's codec signature.
const OGG_HEAD: usize = 64;

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// Whether `bytes` begin with an ID3v2 tag: `ID3`, a major version of 2 to
/// 4, a revision byte, a flags byte whose low four bits are clear, and a
/// size of four bytes of 7 bits.
fn is_id3_tag(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3")
        && (2..=4).contains(&bytes[3])
        && bytes[5] & 0x0f == 0
        && bytes[6..10].iter().all(|byte| byte & 0x80 == 0)
}

/// Whether `bytes` begin with an MPEG Layer III frame header: 11 sync bits,
/// a version that is not reserved, layer III, a bitrate index other than
/// free and bad, and a sample rate index that is not reserved.
fn is_mp3_frame(bytes: &[u8]) -> bool {
    let version = (bytes[1] >> 3) & 0x03;
    let layer = (bytes[1] >> 1) & 0x03;
    let bitrate = bytes[2] >> 4;
    let sample_rate = (bytes[2] >> 2) & 0x03;
    bytes[0] == 0xff
        && bytes[1] & 0xe0 == 0xe0
        && version != 0b01
        && layer == 0b01
        && bitrate != 0 && bitrate != 0x0f
        && sample_rate != 0b11
}

/// How many of their first bytes a NUL byte makes bytes binary within, as
/// git reads them.
const BINARY_NUL_BYTES: usize = 8 * 1024;

/// Whether `bytes` are binary (`crates/plugin-file/command/src/files.rs`).
///
/// Bytes are binary when their first 8 KiB hold a NUL byte, as git and GNU diff
/// decide, or they begin as a media type [`sniff_media_type`] recognizes. Any
/// other bytes are text, and a byte that is not UTF-8 shows as U+FFFD.
///
/// It is the one rule for a job's stdout, edit tracking's copies and line
/// counts, and the first bytes of a longer file answer it as the whole file
/// would.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_NUL_BYTES)].contains(&0) || sniff_media_type(bytes).is_some()
}

/// What a medium record says of the medium whose bytes follow
/// (`crates/command-protocol/src/lib.rs`): its media type,
/// a document's name, and the facts its header gives, which the handler
/// read once: an image's or a video's size in pixels, a video's length.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediumFacts {
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

//! Archive installation: a verified zip archive, or a tar archive compressed
//! with zstd or gzip, unpacked into a directory named by its hash, beside a
//! receipt that lets a later process trust the files without downloading them
//! again.
//!
//! A runner installs a command package's resources into its artifact cache, and
//! the Cloud image build installs into the image, with the same steps
//! (`crates/runner-command-packages/src/cache.rs`,
//! `cloud-guest-image/rootfs/packages.txt`).

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{Digest, Error, InstallLock, coding::ZSTD_MAGIC};

/// The most bytes an installed entry may have: the digest of a larger file
/// stops early, and an entry is written to the disk.
const ENTRY_BYTES: u64 = 1024 * 1024 * 1024;

/// An archive to install: the size and hash it must have, and its entry,
/// the file its user starts, as a path inside it with `/` between the
/// components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    pub digest: Digest,
    pub entry: String,
}

impl Archive {
    /// The entry in an installation at `directory`. The entry is a path in
    /// the archive, whose separator is `/`; it is joined a component at a
    /// time, so that on Windows the path has the system's separators, as
    /// what the runner prints does: `C:\…\chrome-win64\chrome.exe`, not
    /// `C:\…\chrome-win64/chrome.exe`.
    pub fn entry_in(&self, directory: &Path) -> PathBuf {
        directory.join(self.entry.split('/').collect::<PathBuf>())
    }
}

/// What an installation records beside its files: the name of the archive's
/// hash ([`crate::Hash::name`]) and of its entry's SHA-256.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    archive_hash: String,
    entry_hash: String,
}

/// The receipt at `directory`, or none when nothing is there.
async fn receipt(directory: &Path) -> Result<Option<Receipt>, Error> {
    if !tokio::fs::try_exists(directory).await? {
        return Ok(None);
    }
    let invalid = |reason: &str| Error::Installation {
        directory: directory.to_owned(),
        reason: reason.to_owned(),
    };
    let Some(bytes) = crate::receipt::read(directory).await? else {
        return Err(invalid("has no receipt"));
    };
    let receipt = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(&format!("has an invalid receipt: {error}")))?;
    Ok(Some(receipt))
}

/// The entry of the installation of `archive` at `directory` that the installer
/// published, trusted unread: the receipt there names the archive.
///
/// It returns None when nothing is at `directory`; another receipt is an error.
/// Use it for a directory only its installer writes, such as a runner's private
/// cache, where reading the whole entry at each use would cost every start.
pub async fn recorded(directory: &Path, archive: &Archive) -> Result<Option<PathBuf>, Error> {
    let Some(receipt) = receipt(directory).await? else {
        return Ok(None);
    };
    if receipt.archive_hash != archive.digest.hash.name() {
        return Err(Error::Installation {
            directory: directory.to_owned(),
            reason: "holds another archive".to_owned(),
        });
    }
    Ok(Some(archive.entry_in(directory)))
}

/// The installation of `archive` at `directory`: its entry, once the
/// receipt there names the archive and the entry's current hash, or
/// none when nothing is at `directory`. An installation that fails the check
/// is an error rather than a reason to install again or elsewhere: its files
/// changed after it was installed.
pub async fn installed(
    directory: &Path,
    archive: &Archive,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>, Error> {
    let Some(receipt) = receipt(directory).await? else {
        return Ok(None);
    };
    let invalid = |reason: &str| Error::Installation {
        directory: directory.to_owned(),
        reason: reason.to_owned(),
    };
    let entry = archive.entry_in(directory);
    let found = match crate::digest(&entry, ENTRY_BYTES, cancel).await {
        Ok(found) => found,
        Err(Error::TooLarge { .. }) => return Err(invalid("fails its integrity check")),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(invalid("fails its integrity check"));
        }
        Err(error) => return Err(error),
    };
    if receipt.archive_hash != archive.digest.hash.name()
        || receipt.entry_hash != found.hash.name()
    {
        return Err(invalid("fails its integrity check"));
    }
    Ok(Some(entry))
}

/// What [`install_archive`] finds in its root.
pub enum ArchiveInstall {
    /// The installation was there already: its entry.
    Installed(PathBuf),
    /// Nothing was there: the caller writes the archive's verified bytes to
    /// [`Unpacking::archive`], then [`Unpacking::finish`]es.
    Unpack(Unpacking),
}

/// An installation of one archive under way. It holds the archive's install
/// lock, and its temporary directory goes with it, whatever happens.
pub struct Unpacking {
    _lock: InstallLock,
    temporary: tempfile::TempDir,
    destination: PathBuf,
    archive: Archive,
}

/// Starts installing `archive` into the directory of `root` its hash names
/// ([`crate::Hash::name`]); an installation already there is checked, not
/// replaced. Installers of one archive share the lock `<name>.lock` in
/// `root`, so one fetches the archive and the others find its installation.
pub async fn install_archive(
    root: &Path,
    archive: &Archive,
    cancel: &CancellationToken,
) -> Result<ArchiveInstall, Error> {
    let name = archive.digest.hash.name();
    tokio::fs::create_dir_all(root).await?;
    let lock = InstallLock::acquire(&root.join(format!("{name}.lock")), cancel).await?;
    let destination = root.join(name);
    if let Some(entry) = installed(&destination, archive, cancel).await? {
        return Ok(ArchiveInstall::Installed(entry));
    }
    let temporary = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(root)?;
    Ok(ArchiveInstall::Unpack(Unpacking {
        _lock: lock,
        temporary,
        destination,
        archive: archive.clone(),
    }))
}

impl Unpacking {
    /// Where the caller writes the archive, checked against its declared
    /// size and hash as it is written, such as by [`crate::download`].
    pub fn archive(&self) -> PathBuf {
        self.temporary.path().join("archive")
    }

    /// Unpacks the archive the caller wrote, writes the receipt and
    /// publishes the installation; returns its entry.
    pub async fn finish(self, cancel: &CancellationToken) -> Result<PathBuf, Error> {
        let extracted = self.temporary.path().join("extracted");
        extract(&self.archive(), &extracted, cancel).await?;
        publish_installation(&extracted, &self.destination, &self.archive, cancel).await
    }
}

/// Writes the receipt of `archive`'s files, unpacked at `extracted`, and
/// publishes them as `destination`; returns the entry there.
async fn publish_installation(
    extracted: &Path,
    destination: &Path,
    archive: &Archive,
    cancel: &CancellationToken,
) -> Result<PathBuf, Error> {
    let entry = match crate::digest(&archive.entry_in(extracted), ENTRY_BYTES, cancel).await {
        Ok(entry) => entry,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Archive(format!("holds no {}", archive.entry)));
        }
        Err(error) => return Err(error),
    };
    let receipt = Receipt {
        archive_hash: archive.digest.hash.name(),
        entry_hash: entry.hash.name(),
    };
    crate::receipt::write(extracted, &receipt).await?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    crate::publish_directory(extracted, destination).await?;
    Ok(archive.entry_in(destination))
}

/// The kinds of archive an installation unpacks, told apart by their first
/// bytes: the archive's digest, not its name, says which bytes it is.
#[derive(Clone, Copy)]
enum Format {
    Zip,
    Tar(Compression),
}

/// How a tar archive is compressed.
#[derive(Clone, Copy)]
enum Compression {
    /// As the Chrome runtime's
    /// (`crates/plugin-browser/command-chrome/src/driver/installation.rs`).
    Zstd,
    /// As a vendor's CLI may be, such as Antigravity's.
    Gzip,
}

impl Compression {
    /// The tar archive `reader` holds compressed.
    fn decoder<'a>(self, reader: impl Read + 'a) -> std::io::Result<Box<dyn Read + 'a>> {
        Ok(match self {
            Self::Zstd => Box::new(zstd::stream::read::Decoder::new(reader)?),
            // A gzip file may hold several members, whose data follow one
            // another; the single-member decoder would stop after the first.
            Self::Gzip => Box::new(flate2::read::MultiGzDecoder::new(reader)),
        })
    }
}

/// The first bytes of a zip archive's first local file header.
const ZIP_MAGIC: [u8; 4] = *b"PK\x03\x04";

/// The first bytes of a gzip member.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Which kind of archive `file` holds, read from its start, which the file
/// is left at.
fn format(file: &mut File) -> Result<Format, Error> {
    let mut magic = [0_u8; 4];
    let read = file.read_exact(&mut magic);
    file.seek(SeekFrom::Start(0))?;
    match read {
        Ok(()) if magic == ZIP_MAGIC => Ok(Format::Zip),
        Ok(()) if magic == ZSTD_MAGIC => Ok(Format::Tar(Compression::Zstd)),
        Ok(()) if magic[..2] == GZIP_MAGIC => Ok(Format::Tar(Compression::Gzip)),
        Err(error) if error.kind() != std::io::ErrorKind::UnexpectedEof => Err(error.into()),
        _ => Err(Error::Archive(
            "is neither a zip archive nor a tar archive compressed with zstd or gzip".to_owned(),
        )),
    }
}

/// `path` without the `.` components a tar archive may start its paths
/// with, as `./lib/libnss3.so`.
fn tar_path(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, std::path::Component::CurDir))
        .collect()
}

/// Whether the archive at `archive` holds a file at `path`, such as the
/// entry a release record names. Only a zip archive's directory is read; a
/// tar archive is read until the file.
pub async fn holds(archive: &Path, path: &str) -> Result<bool, Error> {
    let archive = archive.to_owned();
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut file = File::open(archive)?;
        match format(&mut file)? {
            Format::Zip => {
                let unreadable = |error: zip::result::ZipError| {
                    Error::Archive(format!("cannot be read: {error}"))
                };
                let mut archive = zip::ZipArchive::new(file).map_err(unreadable)?;
                let Some(index) = archive.index_for_name(&path) else {
                    return Ok(false);
                };
                let entry = archive.by_index_raw(index).map_err(unreadable)?;
                Ok(entry.is_file())
            }
            Format::Tar(compression) => {
                let unreadable =
                    |error: std::io::Error| Error::Archive(format!("cannot be read: {error}"));
                let decoder = compression.decoder(file).map_err(unreadable)?;
                let mut archive = tar::Archive::new(decoder);
                let wanted = Path::new(&path);
                for entry in archive.entries().map_err(unreadable)? {
                    let entry = entry.map_err(unreadable)?;
                    let found = tar_path(&entry.path().map_err(unreadable)?) == wanted;
                    if found {
                        return Ok(entry.header().entry_type().is_file());
                    }
                }
                Ok(false)
            }
        }
    })
    .await
    .map_err(std::io::Error::other)?
}

/// zip and tar check each entry's path, and zip its CRC, but neither has a
/// cancellation of its own; the reader they read through stops when
/// cancelled.
struct Cancellable {
    file: File,
    cancel: CancellationToken,
}

impl Read for Cancellable {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            // Not `Interrupted`: the standard library's readers retry that
            // kind, so extraction would spin instead of stopping.
            return Err(std::io::Error::other("extraction cancelled"));
        }
        self.file.read(buffer)
    }
}

impl Seek for Cancellable {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

/// Extracts the archive at `archive`, of either [`Format`], into
/// `destination` on the blocking pool. Neither format writes outside
/// `destination`: zip refuses such a path, and tar skips it. The call
/// returns only once extraction has stopped, also when cancelled, so the
/// caller alone owns what was extracted.
async fn extract(
    archive: &Path,
    destination: &Path,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    let archive = archive.to_owned();
    let destination = destination.to_owned();
    let reader_cancel = cancel.clone();
    let extracted = tokio::task::spawn_blocking(move || {
        let mut file = File::open(archive)?;
        let format = format(&mut file)?;
        let reader = Cancellable {
            file,
            cancel: reader_cancel,
        };
        match format {
            Format::Zip => {
                let unusable = |error: zip::result::ZipError| {
                    Error::Archive(format!("cannot be extracted: {error}"))
                };
                let mut archive = zip::ZipArchive::new(reader).map_err(unusable)?;
                archive.extract(destination).map_err(unusable)
            }
            Format::Tar(compression) => {
                let unusable =
                    |error: std::io::Error| Error::Archive(format!("cannot be extracted: {error}"));
                let decoder = compression.decoder(reader).map_err(unusable)?;
                std::fs::create_dir(&destination)?;
                tar::Archive::new(decoder)
                    .unpack(&destination)
                    .map_err(unusable)
            }
        }
    })
    .await
    .map_err(std::io::Error::other)?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    extracted
}

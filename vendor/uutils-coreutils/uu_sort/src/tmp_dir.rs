// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore CLOEXEC NONBLOCK RDONLY RUSR unlinkat WRONLY WUSR

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use uucore::context::fs::File;
#[cfg(unix)]
use uucore::context::fs::Permissions;

use uucore::error::UResult;

use crate::SortError;

/// Owns one invocation's temporary directory and removes it on drop.
/// The runner owns the enclosing job directory for forced cancellation cleanup.
/// The directory is only created once the first file is requested.
pub struct TmpDirWrapper {
    dir: Option<Arc<SortTmpDir>>,
    parent_path: PathBuf,
}

impl TmpDirWrapper {
    pub fn new(path: PathBuf) -> Self {
        Self {
            parent_path: uucore::context::resolve(&path),
            dir: None,
        }
    }

    fn init_tmp_dir(&mut self) -> UResult<()> {
        assert!(self.dir.is_none());
        let mut builder = tempfile::Builder::new();
        builder.prefix("uutils_sort");
        // The chunks hold a copy of the input, so keep them out of reach of other
        // users instead of relying on whatever umask the process inherited.
        #[cfg(unix)]
        builder.permissions(Permissions::from_mode(0o700));
        let dir = SortTmpDir::new(&mut builder, &self.parent_path).map_err(|_| {
            SortError::TmpFileCreationFailed {
                path: self.parent_path.clone(),
            }
        })?;
        self.dir = Some(Arc::new(dir));
        Ok(())
    }

    pub fn next_file(&mut self) -> UResult<(File, TmpPath)> {
        if self.dir.is_none() {
            self.init_tmp_dir()?;
        }
        let dir = self.dir.as_ref().unwrap();

        // Count the name first, so cleanup removes the file even if creating it
        // fails after the file exists.
        let index = dir.created.fetch_add(1, Ordering::Relaxed);
        dir.create(index)
            .map_err(|error| SortError::OpenTmpFileFailed { error }.into())
    }
}

impl Drop for TmpDirWrapper {
    fn drop(&mut self) {
        if let Some(dir) = &self.dir
            && let Err(error) = dir.remove()
        {
            uucore::show_error!("failed to remove sort temporary directory: {error}");
        }
    }
}

/// The directory created by [`TmpDirWrapper`], whose files are named `0`, `1`, ...
/// It is only used through a descriptor checked to be this directory, so once its
/// path leads elsewhere, sort neither reads nor removes anything there.
#[cfg(all(unix, not(target_os = "redox")))]
mod imp {
    use rustix::fs::{AtFlags, CWD, Mode, OFlags, openat, unlinkat};
    use uucore::context::fs::File;
    use uucore::context::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::{Builder, TempDir};
    use uucore::fs::FileInfo;

    fn file_id(file: &File) -> io::Result<FileInfo> {
        Ok(FileInfo::from(&file.metadata()?))
    }

    fn open_dir(path: &Path) -> io::Result<File> {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        Ok(openat(CWD, path, flags, Mode::empty())?.into())
    }

    pub struct SortTmpDir {
        temp_dir: TempDir,
        id: FileInfo,
        pub created: AtomicUsize,
    }

    impl SortTmpDir {
        /// Create the directory in `parent`.
        pub fn new(builder: &mut Builder<'_, '_>, parent: &Path) -> io::Result<Self> {
            // `remove` cleans up through the descriptor: `TempDir` would remove
            // the directory recursively by path, wherever that leads by then.
            let temp_dir = builder.disable_cleanup(true).tempdir_in(parent)?;
            // The path is resolved once more right after creating the directory;
            // there is no way to create and open a directory in one step.
            let id = open_dir(temp_dir.path())
                .and_then(|dir| file_id(&dir))
                .inspect_err(|_| {
                    let _ = uucore::context::fs::remove_dir(temp_dir.path());
                })?;
            Ok(Self {
                temp_dir,
                id,
                created: AtomicUsize::new(0),
            })
        }

        /// Open the directory, if its path still leads to it. The descriptor is
        /// only kept for a moment: sort may have none to spare.
        fn open_dir(&self) -> io::Result<File> {
            let dir = open_dir(self.temp_dir.path())?;
            if file_id(&dir)? != self.id {
                return Err(io::ErrorKind::NotFound.into());
            }
            Ok(dir)
        }

        /// Create the file `index`, which must not exist yet.
        pub fn create(self: &Arc<Self>, index: usize) -> io::Result<(File, TmpPath)> {
            let name = index.to_string();
            // Restrict the chunks too, so a directory whose mode is later relaxed
            // doesn't expose them.
            let file = File::from(openat(
                &self.open_dir()?,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )?);
            let tmp_path = TmpPath {
                dir: Arc::clone(self),
                path: self.temp_dir.path().join(name),
                index,
                id: file_id(&file)?,
            };
            Ok((file, tmp_path))
        }

        /// Remove the file `index`, ignoring failures.
        fn remove_file(&self, index: usize) {
            if let Ok(dir) = self.open_dir() {
                let _ = unlinkat(&dir, index.to_string().as_str(), AtFlags::empty());
            }
        }

        /// Remove the files created in the directory, then the directory itself.
        /// Errors while deleting the files are ignored.
        pub fn remove(&self) -> io::Result<()> {
            let dir = self.open_dir()?;
            for index in 0..self.created.load(Ordering::Relaxed) {
                let _ = unlinkat(&dir, index.to_string().as_str(), AtFlags::empty());
            }
            // Only removes an empty directory, and doesn't follow a symlink.
            uucore::context::fs::remove_dir(self.temp_dir.path())
        }
    }

    /// A file created by [`super::TmpDirWrapper::next_file`].
    pub struct TmpPath {
        dir: Arc<SortTmpDir>,
        path: PathBuf,
        index: usize,
        id: FileInfo,
    }

    impl TmpPath {
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Open the file for reading, if it is still the file created. This takes
        /// no descriptor of the directory, as merges reopen files while others are
        /// open.
        pub fn open(&self) -> io::Result<File> {
            // The path may lead elsewhere by now: don't wait for a FIFO there.
            let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
            let file = File::from(openat(CWD, &self.path, flags, Mode::empty())?);
            if file_id(&file)? != self.id {
                return Err(io::ErrorKind::NotFound.into());
            }
            Ok(file)
        }

        /// Delete the file, ignoring failures.
        pub fn remove(self) {
            self.dir.remove_file(self.index);
        }
    }
}

/// The directory created by [`TmpDirWrapper`], whose files are named `0`, `1`, ...
#[cfg(not(all(unix, not(target_os = "redox"))))]
mod imp {
    use uucore::context::fs::{File, OpenOptions};
    use uucore::context::io;
    #[cfg(target_os = "redox")]
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicUsize;
    #[cfg(not(any(target_os = "redox", target_os = "wasi")))]
    use std::sync::atomic::Ordering;
    use tempfile::{Builder, TempDir};

    pub struct SortTmpDir {
        temp_dir: TempDir,
        pub created: AtomicUsize,
    }

    impl SortTmpDir {
        /// Create the directory in `parent`.
        pub fn new(builder: &mut Builder<'_, '_>, parent: &Path) -> io::Result<Self> {
            Ok(Self {
                temp_dir: builder.tempdir_in(parent)?,
                created: AtomicUsize::new(0),
            })
        }

        /// Create the file `index`, which must not exist yet.
        pub fn create(&self, index: usize) -> io::Result<(File, TmpPath)> {
            let path = self.temp_dir.path().join(index.to_string());
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(target_os = "redox")]
            options.mode(0o600);
            let file = options.open(&path)?;
            Ok((file, TmpPath { path }))
        }

        /// Remove the files created in the directory, then the directory itself.
        /// Errors while deleting the files are ignored.
        #[cfg(not(any(target_os = "redox", target_os = "wasi")))]
        pub fn remove(&self) -> io::Result<()> {
            for index in 0..self.created.load(Ordering::Relaxed) {
                let _ = uucore::context::fs::remove_file(self.temp_dir.path().join(index.to_string()));
            }
            uucore::context::fs::remove_dir(self.temp_dir.path())
        }
    }

    /// A file created by [`super::TmpDirWrapper::next_file`].
    pub struct TmpPath {
        path: PathBuf,
    }

    impl TmpPath {
        pub fn path(&self) -> &Path {
            &self.path
        }

        pub fn open(&self) -> io::Result<File> {
            File::open(&self.path)
        }

        /// Delete the file, ignoring failures.
        pub fn remove(self) {
            let _ = uucore::context::fs::remove_file(&self.path);
        }
    }
}

use imp::SortTmpDir;
pub use imp::TmpPath;

#[cfg(all(test, unix))]
mod tests {
    use super::TmpDirWrapper;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn tmp_files_are_private_regardless_of_umask() {
        // Pin a permissive umask: under 0077 the umask alone would produce 0700 and
        // 0600, so the assertions would hold for a broken implementation too.
        // `with_umask` restores it, also on a panic, and the other tests here that
        // create files don't check modes.
        uucore::mode::with_umask(0o022, || {
            let parent = tempfile::tempdir().unwrap();
            let mut wrapper = TmpDirWrapper::new(parent.path().to_owned());
            let (_file, path) = wrapper.next_file().unwrap();

            assert_eq!(mode(path.path().parent().unwrap()), 0o700);
            assert_eq!(mode(path.path()), 0o600);
        });
    }

    /// Once the directory's path leads to another directory, no file is created,
    /// read or removed there, even one with the name of a temporary file.
    #[test]
    #[cfg(not(target_os = "redox"))]
    fn replaced_directory_is_left_alone() {
        let parent = tempfile::tempdir().unwrap();
        let mut wrapper = TmpDirWrapper::new(parent.path().to_owned());
        let (_file, path) = wrapper.next_file().unwrap();
        let tmp_dir = path.path().parent().unwrap().to_owned();
        uucore::context::fs::rename(&tmp_dir, parent.path().join("moved")).unwrap();
        uucore::context::fs::create_dir(&tmp_dir).unwrap();
        uucore::context::fs::write(tmp_dir.join("0"), "keep").unwrap();

        assert!(wrapper.next_file().is_err());
        assert!(path.open().is_err());
        path.remove();
        drop(wrapper);

        assert_eq!(uucore::context::fs::read_to_string(tmp_dir.join("0")).unwrap(), "keep");
        assert!(!tmp_dir.join("1").exists());
    }
}

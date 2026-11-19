//! The files a job opened to write, each one edit: its before side is read
//! as the job first opens it, and its after side once the last copy the job
//! holds of it closes, or when the job ends with a copy still open. The
//! writes between go to the file unrecorded, as a program's writes to its
//! redirected file do.
//!
//! ```text
//! exec 3> f     open:  share 1, before side read
//! echo a >&3    write: straight to f
//! cat x 2> f    open:  share 2 joins the hold; cat ends: share 2 gone
//! exec 3>&-     close: share 1 gone, after side read: one edit
//! ```
//!
//! - **Copies:** a share is an open's, which its copies keep: the duplicates
//!   of a redirection's descriptor, a background task's copy of it, a
//!   utility's `try_clone`, and a program the file is handed to, until it
//!   exits. A utility the shell gives the file to as its output gets the
//!   descriptor without the share, within the command that holds it.
//! - **One file at a time:** a hold's sides are read under the job's lock on
//!   its holds, so an open, a close and the job's end of one file follow each
//!   other, and two of the job's tasks that append to one file make one
//!   chain of edits. A write only asks whether its file is held, of a set
//!   that no reading of a side waits on.
//! - **Another change of a held file:** a whole-file write, a rename or a
//!   removal by one of the job's utilities records its own edit, so the hold
//!   is recorded up to it first, and starts again after it from the file as
//!   it left it (a rename: under the new name; a removal: never).
//! - **A job that ends with copies open** ends their holds; a share that
//!   drops later belongs to a hold that has gone, even when the same file is
//!   held again, which its generation tells.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io,
    path::Path,
    sync::{Arc, Mutex, RwLock},
};

use brush_core::execution_host::FileControl;
use jush_command_sdk::edits::{Held, Recorder};

use crate::scope::{FileIdentity, Scope, file_identity};

/// The holds of one job.
#[derive(Clone, Default)]
pub(crate) struct Holds(Arc<Inner>);

#[derive(Default)]
struct Inner {
    /// The holds by file identity. A std mutex, held while a hold's side is
    /// read: the job's threads open and close files synchronously, and the
    /// recorder's lock is never held while this one is taken.
    table: Mutex<Table>,
    /// The identities held, which a write reads.
    held: RwLock<HashSet<FileIdentity>>,
}

#[derive(Default)]
struct Table {
    holds: HashMap<FileIdentity, Hold>,
    generations: u64,
}

struct Hold {
    /// The before side; none while the file is not to be recorded again,
    /// once removed or replaced, or when it could not be read.
    held: Option<Held>,
    /// The opens whose copies are still open.
    shares: usize,
    generation: u64,
}

impl Holds {
    /// Opens `path` to write with `open`, and a share of the job's hold on
    /// it when it is a regular file: its before side read first, since the
    /// open may truncate it, unless the job holds it already. A path that
    /// is no regular file opens as `record` recorded it before.
    pub(crate) fn open(
        &self,
        scope: &Scope,
        edits: &Recorder,
        path: &Path,
        open: impl FnOnce() -> io::Result<File>,
    ) -> io::Result<(File, Option<Arc<Share>>)> {
        // Opening a pipe can wait for its reader; nothing waits under the
        // lock for that.
        if std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
            return Ok((edits.record(path, open)?, None));
        }
        let mut table = self.0.table.lock().expect("the holds are intact");
        let held = edits.hold(path).ok();
        let file = match held {
            Some(_) => open()?,
            None => edits.record(path, open)?,
        };
        if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
            return Ok((file, None));
        }
        let Ok(identity) = file_identity(&file) else {
            return Ok((file, None));
        };
        let share = self.join(&mut table, scope, identity, held);
        Ok((file, share))
    }

    /// A share of the job's hold on `file`, for a program the file is
    /// handed to, when the job holds it.
    pub(crate) fn share(&self, scope: &Scope, file: &File) -> Option<Arc<Share>> {
        let identity = file_identity(file).ok()?;
        let mut table = self.0.table.lock().expect("the holds are intact");
        table.holds.contains_key(&identity).then(|| self.join(&mut table, scope, identity, None))?
    }

    /// Joins the hold on `identity`, or starts one with `held`.
    fn join(&self, table: &mut Table, scope: &Scope, identity: FileIdentity, held: Option<Held>) -> Option<Arc<Share>> {
        let generation = match table.holds.get_mut(&identity) {
            Some(hold) => {
                hold.shares += 1;
                hold.generation
            }
            None => {
                held.as_ref()?;
                table.generations += 1;
                let generation = table.generations;
                table.holds.insert(identity, Hold { held, shares: 1, generation });
                self.0.held.write().expect("the held set is intact").insert(identity);
                generation
            }
        };
        Some(Arc::new(Share {
            scope: scope.clone(),
            identity,
            generation,
        }))
    }

    /// Whether the job holds `file`, whose writes it then does not record
    /// one by one.
    pub(crate) fn covers(&self, file: &File) -> bool {
        file_identity(file).is_ok_and(|identity| self.0.held.read().expect("the held set is intact").contains(&identity))
    }

    /// Ends one share of the hold on `identity` of `generation`, and the
    /// hold when it was the last, recording its edit.
    fn release(&self, scope: &Scope, identity: FileIdentity, generation: u64) {
        let mut table = self.0.table.lock().expect("the holds are intact");
        let Some(hold) = table.holds.get_mut(&identity).filter(|hold| hold.generation == generation) else {
            return;
        };
        hold.shares -= 1;
        if hold.shares > 0 {
            return;
        }
        let hold = table.holds.remove(&identity).expect("the hold is there");
        self.0.held.write().expect("the held set is intact").remove(&identity);
        if let Some(held) = hold.held {
            scope.settle(held);
        }
    }

    /// Ends every hold, the job having ended with copies open.
    pub(crate) fn drain(&self, scope: &Scope) {
        let mut table = self.0.table.lock().expect("the holds are intact");
        self.0.held.write().expect("the held set is intact").clear();
        for (_, hold) in table.holds.drain() {
            if let Some(held) = hold.held {
                scope.settle(held);
            }
        }
    }

    /// Records the holds on `path` up to now, before another change of it:
    /// the holds to start again once that change is made.
    pub(crate) fn pause(&self, scope: &Scope, path: &Path) -> Paused {
        let mut paused = None;
        // A pipe's open would wait for its writer; only a regular file is
        // held.
        let identity = std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file())
            .then(|| File::open(path).and_then(|file| file_identity(&file)).ok())
            .flatten();
        if let Some(identity) = identity {
            let mut table = self.0.table.lock().expect("the holds are intact");
            if let Some(hold) = table.holds.get_mut(&identity)
                && let Some(held) = hold.held.take()
            {
                scope.settle(held);
                paused = Some((identity, hold.generation));
            }
        }
        Paused { holds: self.clone(), paused }
    }
}

/// Holds recorded up to another change of their file, which start again
/// after it (`Holds::pause`).
pub(crate) struct Paused {
    holds: Holds,
    paused: Option<(FileIdentity, u64)>,
}

impl Paused {
    /// Starts the holds again from the file at `path` as it is now, the file
    /// they hold having stayed there or moved there.
    pub(crate) fn resume(self, edits: &Recorder, path: &Path) {
        let Some((identity, generation)) = self.paused else {
            return;
        };
        let mut table = self.holds.0.table.lock().expect("the holds are intact");
        if let Some(hold) = table.holds.get_mut(&identity).filter(|hold| hold.generation == generation) {
            hold.held = edits.hold(path).ok();
        }
    }
}

/// One open's share of a file's hold, which the copies of that open keep:
/// once the last of them drops, so does the share.
pub(crate) struct Share {
    scope: Scope,
    identity: FileIdentity,
    generation: u64,
}

impl Drop for Share {
    fn drop(&mut self) {
        self.scope.holds.release(&self.scope, self.identity, self.generation);
    }
}

/// The control of a redirection's file the job holds: its scope's, and the
/// share of the hold that the descriptor's duplicates keep.
pub(crate) struct HeldFile {
    pub(crate) control: Arc<dyn FileControl>,
    pub(crate) share: Arc<Share>,
}

impl FileControl for HeldFile {
    fn read(&self, file: &File, buffer: &mut [u8]) -> io::Result<usize> {
        self.control.read(file, buffer)
    }
    fn write(&self, file: &File, buffer: &[u8]) -> io::Result<usize> {
        self.control.write(file, buffer)
    }
    fn duplicate(&self, file: &File) -> io::Result<File> {
        self.control.duplicate(file)
    }
    /// A background task's copy keeps its share: the file stays held until
    /// the task closes it too, or the job ends.
    fn rebound(&self, owner: &Arc<dyn FileControl>) -> Arc<dyn FileControl> {
        Arc::new(Self {
            control: self.control.rebound(owner),
            share: self.share.clone(),
        })
    }
    fn is_host_output(&self) -> bool {
        self.control.is_host_output()
    }
}

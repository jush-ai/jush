// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//! Zero-copy-related functions.

use crate::io::{RawReader, RawWriter, ReadWriteError};
use rustix::pipe::{SpliceFlags, fcntl_setpipe_size};
use crate::context::io::{PipeReader, PipeWriter, Read, Write};
use std::cell::Cell;
use std::os::fd::AsFd;
pub const MAX_ROOTLESS_PIPE_SIZE: usize = 1024 * 1024;
const KERNEL_DEFAULT_PIPE_SIZE: usize = 64 * 1024;

type PipeRes = crate::context::io::Result<Result<(), ()>>;

thread_local! {
    /// Cache empty pipe pair to avoid calling `pipe2` at each copy.
    static PIPE_CACHE: Cell<Option<(PipeReader, PipeWriter)>> = const { Cell::new(None) };
}

/// A type allows to
/// - check that zero-copy succeed by `?.is_ok()`
/// - check that zero-copy failed, but read/write fallback succeed by `?.is_err()`
/// - catch the read/write fallback's error by ? or let Err(e)
///
/// Whether the zero-copy paths run: not in an invocation under its owner's
/// control (`crate::context::control`), whose reads and writes go through
/// the context for its cancellation. A caller asks before it grows a pipe it
/// does not own for splicing: a grown pipe that is then written to normally
/// only holds more, up to 1 MiB, so the writer keeps writing after its
/// reader has gone, where a 64 KiB pipe would end it with EPIPE.
#[inline]
pub fn zero_copy() -> bool {
    crate::context::control().is_none()
}

/// use `rustix::io::Result` for functions without read/write fallback
type SpliceRes = Result<Result<(), ()>, ReadWriteError>;

/// return pipe and try to extend its size
/// `SIZE_REQUIRED` should be true if you want to fail when changing pipe size failed
/// e.g. writing size to pipe should not hang
/// `SIZE_REQUIRED=false` allows to continue unbuffered splice I/O with default pipe size even if fcntl failed
///
/// used for resolving the limitation for splice: one of a input or output should be pipe
#[inline]
pub fn pipe<const SIZE_REQUIRED: bool>() -> crate::context::io::Result<(PipeReader, PipeWriter)> {
    let pair = crate::context::io::pipe()?;
    // pipe size is not RAM consumed by pipe with zero-copy. So we never use other size
    let r = fcntl_setpipe_size(&pair.0, MAX_ROOTLESS_PIPE_SIZE);
    if SIZE_REQUIRED {
        r?;
    }

    Ok(pair)
}

/// Less noisy wrapper around splice syscall
///
/// Up to `len` bytes are moved from `source` to `target`. Returns the number
/// of successfully moved bytes.
///
/// splice fails if both of `source` and `target` are not pipe. Consider using
/// `splice_unbounded_broker` or `splice_unbounded_auto` in the case.
#[inline]
pub fn splice(source: &impl AsFd, target: &impl AsFd, len: usize) -> rustix::io::Result<usize> {
    crate::context::check_cancelled();
    if !zero_copy() {
        // The caller's read/write fallback preserves execution-owned cancellation.
        return Err(rustix::io::Errno::NOSYS);
    }
    rustix::pipe::splice(source, None, target, None, len, SpliceFlags::empty())
}

/// splice `len` bytes from `pipe` into `dest`.
///
/// On error, `pipe` may still hold bytes that were not written: don't reuse it.
#[inline]
pub fn drain_pipe(pipe: &PipeReader, dest: &impl AsFd, len: usize) -> PipeRes {
    debug_assert!(len <= MAX_ROOTLESS_PIPE_SIZE, "unexpected RAM usage");
    // 1st error is used to detect missing support for splice
    let mut remaining = if let Ok(s) = splice(pipe, dest, len) {
        len - s
    } else {
        // read/write fallback
        // use read_to_end to make pipe empty for the case write failed
        let mut drain = Vec::with_capacity(len);
        pipe.take(len as u64).read_to_end(&mut drain)?;
        RawWriter(&dest).write_all(&drain)?;
        return Ok(Err(()));
    };
    // GNU cat catches all strace injections for 2nd+ splice
    while remaining > 0 {
        remaining -= splice(pipe, dest, remaining)?;
    }
    Ok(Ok(()))
}

/// force-splice source to dest even both of them are not pipe via broker pipe
///
/// throughput is better than direct splice for the case one of in/output is pipe by unknown reason
/// This includes read ahead and optimization for stdout's pipe size
#[inline]
pub fn splice_unbounded_auto(source: &impl AsFd, dest: &mut impl AsFd) -> SpliceRes {
    if !zero_copy() {
        // The caller's read/write fallback, into `dest` as it is.
        return Ok(Err(()));
    }
    let Ok((pipe_rd, pipe_wr)) = PIPE_CACHE.take().map_or_else(pipe::<false>, Ok) else {
        return Ok(Err(()));
    };

    // fcntl for input would not improve throughput since
    // - sender with splice probably increased size already
    // - sender without splice is bottleneck
    let _ = fcntl_setpipe_size(&mut *dest, MAX_ROOTLESS_PIPE_SIZE);
    // pre-generate page caches for splice
    let _ = rustix::fs::fadvise(source, 0, None, rustix::fs::Advice::Sequential);
    // 1st error is used to detect missing support for splice
    let res = match splice(&source, &pipe_wr, MAX_ROOTLESS_PIPE_SIZE) {
        Ok(0) => Ok(()),
        Ok(n) => {
            if drain_pipe(&pipe_rd, dest, n)
                .map_err(ReadWriteError::Write)?
                .is_err()
            {
                Err(())
            } else {
                // GNU cat catches all strace injections for 2nd+ splice
                while let mut n @ 1.. = splice(&source, &pipe_wr, MAX_ROOTLESS_PIPE_SIZE)
                    .map_err(|error| ReadWriteError::Read(error.into()))?
                {
                    while n > 0 {
                        n -= splice(&pipe_rd, dest, n)
                            .map_err(|error| ReadWriteError::Write(error.into()))?;
                    }
                }
                Ok(())
            }
        }
        Err(_) => Err(()),
    };
    // pipe is empty. Then we can reuse it at next splice.
    PIPE_CACHE.set(Some((pipe_rd, pipe_wr)));
    Ok(res)
}

/// splice `n` bytes with read/write fallback
/// return actually sent bytes
#[inline]
pub fn send_n_bytes(input: impl AsFd, target: impl AsFd, n: u64) -> crate::context::io::Result<u64> {
    if !zero_copy() {
        return crate::context::io::copy(&mut RawReader(input).take(n), &mut RawWriter(target));
    }
    let pipe_size = n.min(MAX_ROOTLESS_PIPE_SIZE as u64) as usize;
    let resize = pipe_size > KERNEL_DEFAULT_PIPE_SIZE;
    // improve throughput if output is pipe
    // expected that input is already extended if it is coming from splice
    if resize {
        let _ = fcntl_setpipe_size(&target, pipe_size);
    }
    let Some((broker_r, broker_w)) = PIPE_CACHE.take().or_else(|| {
        // use crate::context::io::pipe to avoid fcntl when it is not useful
        let pair = crate::context::io::pipe().ok()?;
        if resize {
            let _ = fcntl_setpipe_size(&pair.0, pipe_size);
        }
        Some(pair)
    }) else {
        return crate::context::io::copy(&mut RawReader(input).take(n), &mut RawWriter(target));
    };
    let mut n = n;
    let mut bytes_written: u64 = 0;
    while n > 0 {
        match splice(&input, &broker_w, usize::try_from(n).unwrap_or(usize::MAX)) {
            Ok(0) => {
                PIPE_CACHE.set(Some((broker_r, broker_w)));
                return Ok(bytes_written);
            }
            Ok(s) => {
                n -= s as u64;
                bytes_written += s as u64;
                if drain_pipe(&broker_r, &target, s)?.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    // pipe is empty. Then we can reuse it at next splice.
    PIPE_CACHE.set(Some((broker_r, broker_w)));
    // remove buffering from this fallback by RawReader, or order of output would be wrong with multiple input
    bytes_written +=
        crate::context::io::copy(&mut RawReader(input).take(n), &mut RawWriter(target))?;
    Ok(bytes_written)
}

/// discard `n` bytes by splice
/// return actually discarded bytes
/// Err(b) means we discarded b bytes, but we should try to discarding remaining bytes by read
#[inline]
pub fn discard_n_bytes(fd: impl AsFd, n: usize) -> Result<usize, usize> {
    if !zero_copy() {
        // Report zero consumed bytes so the caller uses its controlled reader.
        return Err(0);
    }
    let mut discarded = 0;
    let dev_null = dev_null().ok_or(0_usize)?;
    while discarded < n
        && let Ok(s) = splice(&fd, &dev_null, n - discarded)
    {
        if s == 0 {
            return Ok(discarded);
        }
        discarded += s;
    }
    // else, input is not a pipe
    let (pipe_read, pipe_write) = pipe::<false>().map_err(|_| discarded)?;
    while discarded < n
        && let Ok(s @ 1..) = splice(&fd, &pipe_write, n - discarded)
    {
        discarded += s;
        // pipe to null is not blocked. So this returns the same length at most cases
        // next splice does not hang if we discarded 1+ pages
        splice(&pipe_read, &dev_null, s).map_err(|_| discarded)?;
    }

    Ok(discarded)
}

/// Return verified /dev/null
///
/// `splice` to /dev/null is faster than `read` when we skip or count the non-seekable input
#[inline]
pub fn dev_null() -> Option<crate::context::fs::File> {
    let null = crate::context::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .ok()?;
    let stat = rustix::fs::fstat(&null).ok()?;
    let dev = stat.st_rdev;
    ((rustix::fs::major(dev), rustix::fs::minor(dev)) == (1, 3)).then_some(null)
}

// Less noisy wrapper around tee syscall
#[inline]
pub fn tee(source: &impl AsFd, target: &impl AsFd, len: usize) -> rustix::io::Result<usize> {
    rustix::pipe::tee(source, target, len, SpliceFlags::empty())
}

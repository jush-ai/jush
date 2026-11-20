//! A file's size and hash, and the check of bytes against a declared one.
//!
//! Jush hashes what it builds with SHA-256. A file another publisher hashes,
//! such as a vendor's CLI with SHA-512, is checked by the publisher's own
//! hash, so nothing in between downloads and hashes it again.

use std::{io::Read, path::Path};

use sha2::{Digest as _, Sha256, Sha512};
use tokio_util::sync::CancellationToken;

use crate::Error;

/// A file's size and its hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Digest {
    pub size: u64,
    pub hash: Hash,
}

impl Digest {
    /// A file's size and its SHA-256 in lowercase hex.
    pub fn sha256(size: u64, hex: impl Into<String>) -> Self {
        Self {
            size,
            hash: Hash::Sha256(hex.into()),
        }
    }
}

/// A hash in lowercase hex, by its algorithm.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Hash {
    Sha256(String),
    Sha512(String),
}

/// The prefix of a SHA-512's [`Hash::name`].
const SHA512_NAME: &str = "sha512-";

impl Hash {
    /// The hex.
    pub fn hex(&self) -> &str {
        match self {
            Self::Sha256(hex) | Self::Sha512(hex) => hex,
        }
    }

    /// The SHA-256's hex, when this is one.
    pub fn sha256(&self) -> Option<&str> {
        match self {
            Self::Sha256(hex) => Some(hex),
            Self::Sha512(_) => None,
        }
    }

    /// The name the hash gives what it identifies, such as a cache entry or
    /// an installation's directory: a SHA-256's hex, as every such name was
    /// before another algorithm, or `sha512-<hex>`.
    pub fn name(&self) -> String {
        match self {
            Self::Sha256(hex) => hex.clone(),
            Self::Sha512(hex) => format!("{SHA512_NAME}{hex}"),
        }
    }

    /// The hash a [`Self::name`] stands for.
    pub fn named(name: &str) -> Self {
        match name.strip_prefix(SHA512_NAME) {
            Some(hex) => Self::Sha512(hex.to_owned()),
            None => Self::Sha256(name.to_owned()),
        }
    }
}

/// A hash under way, of the algorithm of a [`Hash`].
#[derive(Debug)]
enum Hashing {
    Sha256(Sha256),
    Sha512(Sha512),
}

impl Hashing {
    fn like(hash: &Hash) -> Self {
        match hash {
            Hash::Sha256(_) => Self::Sha256(Sha256::new()),
            Hash::Sha512(_) => Self::Sha512(Sha512::new()),
        }
    }

    fn update(&mut self, chunk: &[u8]) {
        match self {
            Self::Sha256(hashing) => hashing.update(chunk),
            Self::Sha512(hashing) => hashing.update(chunk),
        }
    }

    fn finish(self) -> Hash {
        match self {
            Self::Sha256(hashing) => Hash::Sha256(format!("{:x}", hashing.finalize())),
            Self::Sha512(hashing) => Hash::Sha512(format!("{:x}", hashing.finalize())),
        }
    }
}

/// Counts and hashes bytes as they arrive; bytes past the limit fail at once.
#[derive(Debug)]
struct Measure {
    hash: Hashing,
    size: u64,
    limit: u64,
}

impl Measure {
    fn new(hash: Hashing, limit: u64) -> Self {
        Self {
            hash,
            size: 0,
            limit,
        }
    }

    fn update(&mut self, chunk: &[u8]) -> Result<(), Error> {
        self.size = self
            .size
            .checked_add(chunk.len() as u64)
            .filter(|size| *size <= self.limit)
            .ok_or(Error::TooLarge {
                declared: self.limit,
            })?;
        self.hash.update(chunk);
        Ok(())
    }

    /// The size and hash of the bytes seen.
    fn finish(self) -> Digest {
        Digest {
            size: self.size,
            hash: self.hash.finish(),
        }
    }
}

/// Checks bytes against a declared digest as they arrive, so a download that
/// grows past its size stops at once.
#[derive(Debug)]
pub struct Verifier {
    expected: Digest,
    measure: Measure,
}

impl Verifier {
    pub fn new(expected: &Digest) -> Self {
        Self {
            expected: expected.clone(),
            measure: Measure::new(Hashing::like(&expected.hash), expected.size),
        }
    }

    /// Counts and hashes the next chunk; bytes past the declared size fail.
    pub fn update(&mut self, chunk: &[u8]) -> Result<(), Error> {
        self.measure.update(chunk)
    }

    /// Whether the bytes seen are exactly the declared ones.
    pub fn finish(self) -> Result<(), Error> {
        let seen = self.measure.finish();
        if seen.size != self.expected.size {
            return Err(Error::Size {
                declared: self.expected.size,
                actual: seen.size,
            });
        }
        if seen.hash != self.expected.hash {
            return Err(Error::Digest);
        }
        Ok(())
    }
}

/// Hashes the file at `path` by SHA-256 on the blocking pool; a file longer
/// than `limit` bytes fails without being read to its end.
pub async fn digest(path: &Path, limit: u64, cancel: &CancellationToken) -> Result<Digest, Error> {
    let path = path.to_owned();
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(path)?;
        let mut measure = Measure::new(Hashing::Sha256(Sha256::new()), limit);
        let mut buffer = vec![0; 64 * 1024];
        loop {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            measure.update(&buffer[..count])?;
        }
        Ok(measure.finish())
    })
    .await
    .map_err(std::io::Error::other)?
}

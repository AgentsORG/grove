//! The object store: immutable, content-addressed files under `.grove/objects`.
//!
//! Objects are written once to a temporary file and renamed into place, so a
//! crash never leaves a half-written object at a valid address. Reads verify
//! the address, so a corrupted object is reported, never silently used.

use crate::hash::Hash;
use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into() }
    }

    fn path(&self, h: &Hash) -> PathBuf {
        let hex = h.hex();
        self.dir.join(&hex[..2]).join(&hex[2..])
    }

    /// Whether an object is already stored.
    pub fn has(&self, h: &Hash) -> bool {
        self.path(h).exists()
    }

    /// Store raw bytes under a domain tag; returns their address.
    pub fn put_bytes(&self, domain: &str, bytes: &[u8]) -> Result<Hash> {
        let h = Hash::of(domain, bytes);
        let p = self.path(&h);
        if !p.exists() {
            write_atomic(&p, bytes)?;
        }
        Ok(h)
    }

    /// Read raw bytes and check they still hash to their address.
    pub fn get_bytes(&self, domain: &str, h: &Hash) -> Result<Vec<u8>> {
        let p = self.path(h);
        let bytes = fs::read(&p).with_context(|| format!("missing object {h}"))?;
        if Hash::of(domain, &bytes) != *h {
            bail!("object {h} is corrupt (its content no longer matches its address)");
        }
        Ok(bytes)
    }

    pub fn put_json<T: Serialize>(&self, domain: &str, value: &T) -> Result<Hash> {
        self.put_bytes(domain, &serde_json::to_vec(value)?)
    }

    pub fn get_json<T: DeserializeOwned>(&self, domain: &str, h: &Hash) -> Result<T> {
        let bytes = self.get_bytes(domain, h)?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("object {h} is not a valid {domain}"))
    }

    /// Store one declaration's source text.
    pub fn put_decl(&self, source: &str) -> Result<Hash> {
        self.put_bytes("decl", source.as_bytes())
    }

    pub fn get_decl(&self, h: &Hash) -> Result<String> {
        Ok(String::from_utf8(self.get_bytes("decl", h)?)?)
    }

    /// Every object's address, for `grove verify`.
    pub fn all(&self) -> Result<Vec<PathBuf>> {
        let mut out = vec![];
        if !self.dir.exists() {
            return Ok(out);
        }
        for shard in fs::read_dir(&self.dir)? {
            let shard = shard?.path();
            if shard.is_dir() {
                for obj in fs::read_dir(&shard)? {
                    out.push(obj?.path());
                }
            }
        }
        out.sort();
        Ok(out)
    }
}

/// Write a file so readers see either the old bytes or the new, never half.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), n));
    fs::write(&tmp, bytes)?;
    // On Windows rename fails if the target exists; objects are immutable, so
    // an existing target already holds these bytes.
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => {
            let _ = fs::remove_file(path);
            fs::rename(&tmp, path).with_context(|| format!("could not write {}", path.display()))
        }
        Err(e) => Err(e).with_context(|| format!("could not write {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_and_detect_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path());
        let h = s.put_decl("fn a() {}\n").unwrap();
        assert_eq!(s.get_decl(&h).unwrap(), "fn a() {}\n");
        // the same content lands at the same address
        assert_eq!(s.put_decl("fn a() {}\n").unwrap(), h);
        fs::write(s.path(&h), b"tampered").unwrap();
        assert!(s.get_decl(&h).is_err());
    }
}

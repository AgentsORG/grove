//! Content addresses.
//!
//! Every object Grove stores is addressed by a SHA-256 of its bytes, prefixed
//! with a domain tag ("decl", "snapshot", "change", ...) so two kinds of object
//! with the same bytes never share an address. The algorithm is part of the
//! string ("sha256:<hex>"), so a later move to another hash can coexist with
//! old objects: the lesson of git's long SHA-1 to SHA-256 transition.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Hash(String);

impl Hash {
    pub const ALGORITHM: &'static str = "sha256";

    /// Hash `bytes` under a domain tag.
    pub fn of(domain: &str, bytes: &[u8]) -> Hash {
        let mut h = Sha256::new();
        h.update(domain.as_bytes());
        h.update([0u8]);
        h.update(bytes);
        Hash(format!("{}:{}", Self::ALGORITHM, hex::encode(h.finalize())))
    }

    /// Parse a stored address, checking its shape.
    pub fn parse(s: &str) -> Option<Hash> {
        let hex = s.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| Hash(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The hex digest without the algorithm tag.
    pub fn hex(&self) -> &str {
        self.0.split_once(':').map(|(_, h)| h).unwrap_or(&self.0)
    }

    /// Eight hex characters, for people.
    pub fn short(&self) -> &str {
        &self.hex()[..8]
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The address of one declaration's source text.
pub fn decl_hash(source: &str) -> Hash {
    Hash::of("decl", source.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_separate_equal_bytes() {
        assert_ne!(Hash::of("decl", b"x"), Hash::of("change", b"x"));
        assert_eq!(Hash::of("decl", b"x"), Hash::of("decl", b"x"));
    }

    #[test]
    fn parse_round_trips() {
        let h = decl_hash("fn a() {}\n");
        assert_eq!(Hash::parse(h.as_str()), Some(h.clone()));
        assert_eq!(h.short().len(), 8);
        assert!(Hash::parse("sha1:abc").is_none());
    }
}

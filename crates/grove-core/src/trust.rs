//! Who did it, and who said yes.
//!
//! - **Keys.** Every person and agent can have an ed25519 key, kept in
//!   `~/.grove/keys` (or `$GROVE_HOME/keys`). Changes are signed when they
//!   are recorded; the repository's config lists the keys it trusts.
//! - **Review rules.** Some changes need a person before they land: a public
//!   signature changed, a heavily linked declaration rewritten, new file,
//!   network or process access, or a protected path. A person approves a
//!   specific version of a change; a new version needs a new approval.

use crate::graph::View;
use crate::model::{Actor, ActorKind, Change, DeclKind, Op};
use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// A signature over a change (everything in it except the signature).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// The signer's public key, hex.
    pub key: String,
    /// The ed25519 signature, hex.
    pub sig: String,
}

/// The rules a repository applies before anything lands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    /// Refuse changes that aren't signed by a trusted key.
    #[serde(default)]
    pub require_signed: bool,
    /// A public function's signature changed: a person approves.
    #[serde(default)]
    pub signature: bool,
    /// New file, network, process or HTTP access: a person approves.
    #[serde(default)]
    pub capabilities: bool,
    /// Rewriting a declaration with at least this many links into it: a
    /// person approves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_links: Option<usize>,
    /// Paths (prefixes) whose changes a person approves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected: Vec<String>,
}

/// A person's yes to one version of a change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub by: Actor,
    pub version: u32,
    pub at: u64,
    /// The approver's signature over (change id, version), when they have a
    /// key. A repository that requires signatures only counts signed ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
}

fn approval_payload(change_id: &str, version: u32, by: &Actor) -> Vec<u8> {
    format!("grove-approval\0{change_id}\0{version}\0{}", actor_spec(by)).into_bytes()
}

/// Sign an approval with the approver's key, if they have one.
pub fn sign_approval(change_id: &str, a: &mut Approval) {
    if let Some(k) = signing_key(&a.by) {
        let sig = k.sign(&approval_payload(change_id, a.version, &a.by));
        a.signature = Some(Signature {
            key: hex::encode(k.verifying_key().to_bytes()),
            sig: hex::encode(sig.to_bytes()),
        });
    }
}

/// Does this approval count? Without `require_signed`, every approval does;
/// with it, only one signed by a key the repository trusts for that person.
pub fn approval_counts(
    change_id: &str,
    a: &Approval,
    trusted_keys: &BTreeMap<String, String>,
    require_signed: bool,
) -> bool {
    if a.by.kind != ActorKind::Person {
        return false;
    }
    if !require_signed {
        return true;
    }
    let Some(s) = &a.signature else { return false };
    if trusted_keys.get(&actor_spec(&a.by)) != Some(&s.key) {
        return false;
    }
    let ok = (|| -> Option<()> {
        let key: [u8; 32] = hex::decode(&s.key).ok()?.try_into().ok()?;
        let sig: [u8; 64] = hex::decode(&s.sig).ok()?.try_into().ok()?;
        VerifyingKey::from_bytes(&key)
            .ok()?
            .verify(
                &approval_payload(change_id, a.version, &a.by),
                &ed25519_dalek::Signature::from_bytes(&sig),
            )
            .ok()
    })();
    ok.is_some()
}

/// `agent:codex`, `person:meera`: how config names an actor.
pub fn actor_spec(a: &Actor) -> String {
    match a.kind {
        ActorKind::Agent => format!("agent:{}", a.name),
        ActorKind::Person => format!("person:{}", a.name),
    }
}

fn grove_home() -> PathBuf {
    if let Ok(h) = std::env::var("GROVE_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".grove")
}

fn key_path(actor: &Actor) -> PathBuf {
    let slug: String = actor_spec(actor)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    grove_home().join("keys").join(format!("{slug}.json"))
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    actor: String,
    public: String,
    secret: String,
}

/// Make a key for an actor (or return the one it has). Returns the public key.
pub fn new_key(actor: &Actor) -> Result<String> {
    if let Some(k) = public_key(actor) {
        return Ok(k);
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| anyhow::anyhow!("no randomness for a key: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    let public = hex::encode(key.verifying_key().to_bytes());
    let file = KeyFile {
        actor: actor_spec(actor),
        public: public.clone(),
        secret: hex::encode(seed),
    };
    let path = key_path(actor);
    crate::store::write_atomic(&path, &serde_json::to_vec_pretty(&file)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(public)
}

pub fn public_key(actor: &Actor) -> Option<String> {
    let f: KeyFile = serde_json::from_slice(&std::fs::read(key_path(actor)).ok()?).ok()?;
    Some(f.public)
}

fn signing_key(actor: &Actor) -> Option<SigningKey> {
    let f: KeyFile = serde_json::from_slice(&std::fs::read(key_path(actor)).ok()?).ok()?;
    let seed: [u8; 32] = hex::decode(f.secret).ok()?.try_into().ok()?;
    Some(SigningKey::from_bytes(&seed))
}

fn payload(change: &Change) -> Vec<u8> {
    let mut c = change.clone();
    c.signature = None;
    serde_json::to_vec(&c).unwrap_or_default()
}

/// Sign a change with its actor's key, if the actor has one.
pub fn sign(change: &mut Change) -> bool {
    match signing_key(&change.actor) {
        Some(k) => {
            change.signature = None;
            let sig = k.sign(&payload(change));
            change.signature = Some(Signature {
                key: hex::encode(k.verifying_key().to_bytes()),
                sig: hex::encode(sig.to_bytes()),
            });
            true
        }
        None => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigStatus {
    Unsigned,
    /// The signature matches the change. `trusted` when the repository lists
    /// this key for this actor.
    Valid {
        trusted: bool,
        key: String,
    },
    /// The change was altered after it was signed, or the signature is junk.
    Invalid,
}

pub fn verify(change: &Change, trusted_keys: &BTreeMap<String, String>) -> SigStatus {
    let Some(s) = &change.signature else {
        return SigStatus::Unsigned;
    };
    let ok = (|| -> Result<()> {
        let key: [u8; 32] = hex::decode(&s.key)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("key length"))?;
        let sig: [u8; 64] = hex::decode(&s.sig)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("signature length"))?;
        VerifyingKey::from_bytes(&key)
            .context("bad key")?
            .verify(
                &payload(change),
                &ed25519_dalek::Signature::from_bytes(&sig),
            )
            .context("bad signature")?;
        Ok(())
    })();
    match ok {
        Ok(()) => SigStatus::Valid {
            trusted: trusted_keys.get(&actor_spec(&change.actor)) == Some(&s.key),
            key: s.key.clone(),
        },
        Err(_) => SigStatus::Invalid,
    }
}

fn header(source: &str) -> &str {
    source
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("//"))
        .unwrap_or("")
}

const CAPABILITIES: &[&str] = &["std.fs", "std.net", "std.http", "std.proc"];

/// Why this change needs a person, if it does.
pub fn reasons(change: &Change, before: &View, after: &View, review: &Review) -> Vec<String> {
    let mut out = vec![];
    let ids: BTreeSet<&String> = change.ops.iter().map(Op::target).collect();
    let incoming = |id: &String| before.edges().iter().filter(|e| &e.to == id).count();
    for id in ids {
        let (b, a) = (before.decls.get(id), after.decls.get(id));
        let node = a.or(b);
        let name = node.map(|n| n.entry.name.clone()).unwrap_or_default();
        if review.signature {
            if let (Some(b), Some(a)) = (b, a) {
                let (hb, ha) = (header(&b.source), header(&a.source));
                if b.entry.kind == DeclKind::Fn
                    && hb != ha
                    && (hb.starts_with("pub ") || ha.starts_with("pub "))
                {
                    out.push(format!("public signature of {name} changed"));
                }
            }
        }
        if review.capabilities {
            if let Some(a) = a {
                for cap in CAPABILITIES {
                    if a.source.contains(cap) && !b.map(|b| b.source.contains(cap)).unwrap_or(false)
                    {
                        out.push(format!("{name} now uses {cap}"));
                    }
                }
            }
        }
        if let Some(n) = review.hot_links {
            let k = incoming(id);
            if b.is_some() && k >= n {
                out.push(format!("{name} has {k} links into it"));
            }
        }
        if let Some(node) = node {
            for p in &review.protected {
                let p = p.trim_end_matches('/');
                if node.entry.file == p || node.entry.file.starts_with(&format!("{p}/")) {
                    out.push(format!("{} is protected", node.entry.file));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Can this actor approve this change? A person, and not the person who
/// made it (an agent's change can be approved by the person it worked for).
pub fn can_approve(by: &Actor, change: &Change) -> Result<()> {
    if by.kind != ActorKind::Person {
        bail!("only a person can approve; {} is an agent", by.name);
    }
    if change.actor.kind == ActorKind::Person && change.actor.name == by.name {
        bail!("{} made this change, so someone else approves it", by.name);
    }
    Ok(())
}

fn tag_payload(name: &str, landing: &crate::hash::Hash) -> Vec<u8> {
    format!("grove-tag\0{name}\0{landing}").into_bytes()
}

/// Sign a tag with the tagger's key, if they have one.
pub fn sign_tag(name: &str, landing: &crate::hash::Hash, by: &Actor) -> Option<Signature> {
    let k = signing_key(by)?;
    let sig = k.sign(&tag_payload(name, landing));
    Some(Signature {
        key: hex::encode(k.verifying_key().to_bytes()),
        sig: hex::encode(sig.to_bytes()),
    })
}

/// Check a tag's signature. `Ok(who)` names the trusted actor whose key
/// signed it; `Err` means it doesn't verify, or the key isn't trusted.
pub fn verify_tag(
    name: &str,
    landing: &crate::hash::Hash,
    sig: &Signature,
    trusted_keys: &BTreeMap<String, String>,
) -> Result<String, String> {
    let check = || -> Option<bool> {
        let key: [u8; 32] = hex::decode(&sig.key).ok()?.try_into().ok()?;
        let s: [u8; 64] = hex::decode(&sig.sig).ok()?.try_into().ok()?;
        Some(
            VerifyingKey::from_bytes(&key)
                .ok()?
                .verify(
                    &tag_payload(name, landing),
                    &ed25519_dalek::Signature::from_bytes(&s),
                )
                .is_ok(),
        )
    };
    if check() != Some(true) {
        return Err("signature doesn't match".into());
    }
    trusted_keys
        .iter()
        .find(|(_, k)| **k == sig.key)
        .map(|(who, _)| who.clone())
        .ok_or_else(|| format!("signed by an untrusted key {}", &sig.key[..12]))
}

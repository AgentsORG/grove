//! The records Grove keeps: declarations, snapshots of the program graph,
//! changes with their footprints, landings on main, and actors.

use crate::hash::Hash;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A declaration's stable identity. It survives renames and edits; only the
/// content address changes.
pub type DeclId = String;

/// A change's stable identity. It survives new versions of the same change.
pub type ChangeId = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeclKind {
    /// A function.
    Fn,
    /// A top-level constant.
    Const,
    /// A `use` line.
    Use,
    /// A test block.
    Test,
    /// A shape, enum, choice or type.
    Type,
    /// A whole file that is not Zero code (docs, config, other languages).
    File,
    /// A binary file (an image, a font, an archive), stored as base64.
    Binary,
    /// Anything else at the top level of a Zero file.
    Other,
}

impl DeclKind {
    /// Kinds other declarations can refer to by name.
    pub fn is_referable(self) -> bool {
        matches!(self, DeclKind::Fn | DeclKind::Const | DeclKind::Type)
    }
}

/// One node of the program graph, as stored in a snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub kind: DeclKind,
    pub file: String,
    pub order: u64,
    pub content: Hash,
}

/// The whole program graph at one moment: every declaration's identity,
/// place and content address. Edges are derived from content, so they are
/// never stored and can never disagree with it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub decls: BTreeMap<DeclId, Entry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Person,
    Agent,
}

/// Who made a change. An agent always acts for a person when one is named.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_person: Option<String>,
}

impl Actor {
    pub fn person(name: &str) -> Actor {
        Actor {
            kind: ActorKind::Person,
            name: name.to_string(),
            for_person: None,
        }
    }

    pub fn agent(name: &str, for_person: Option<&str>) -> Actor {
        Actor {
            kind: ActorKind::Agent,
            name: name.to_string(),
            for_person: for_person.map(str::to_string),
        }
    }

    /// Parse `agent:codex`, `person:meera` or a bare name (a person).
    pub fn parse(spec: &str, for_person: Option<&str>) -> Actor {
        match spec.split_once(':') {
            Some(("agent", name)) => Actor::agent(name, for_person),
            Some(("person", name)) => Actor {
                for_person: for_person.map(str::to_string),
                ..Actor::person(name)
            },
            _ => Actor {
                for_person: for_person.map(str::to_string),
                ..Actor::person(spec)
            },
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        if let Some(p) = &self.for_person {
            write!(f, " for {p}")?;
        }
        Ok(())
    }
}

/// One edit to the program graph. Ops address declarations by identity and
/// carry what they expect to find, so a stale op fails instead of guessing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    Add {
        id: DeclId,
        name: String,
        kind: DeclKind,
        file: String,
        order: u64,
        source: String,
    },
    Replace {
        id: DeclId,
        expect: Hash,
        source: String,
    },
    /// Rename a declaration and every reference to it, wherever those
    /// references are when the op is applied, including ones added later.
    Rename {
        id: DeclId,
        from: String,
        to: String,
    },
    Delete {
        id: DeclId,
        expect: Hash,
    },
}

impl Op {
    pub fn target(&self) -> &DeclId {
        match self {
            Op::Add { id, .. }
            | Op::Replace { id, .. }
            | Op::Rename { id, .. }
            | Op::Delete { id, .. } => id,
        }
    }
}

/// What a change touches. `writes` are declarations whose content it sets;
/// `rewrites` are references a rename updates (these commute with other
/// edits); `reads` are the neighbours whose meaning it depends on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    pub writes: BTreeSet<DeclId>,
    #[serde(default)]
    pub rewrites: BTreeSet<DeclId>,
    pub reads: BTreeSet<DeclId>,
}

impl Footprint {
    /// Everything this change's result depends on.
    pub fn touches(&self) -> BTreeSet<DeclId> {
        self.writes
            .iter()
            .chain(&self.rewrites)
            .chain(&self.reads)
            .cloned()
            .collect()
    }

    /// Everything this change modifies.
    pub fn modifies(&self) -> BTreeSet<DeclId> {
        self.writes.iter().chain(&self.rewrites).cloned().collect()
    }
}

/// A unit of work: the edits, why, who, and what they touch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub id: ChangeId,
    /// Versions of one change share an id; each new recording bumps this.
    pub version: u32,
    /// The landing this change was made on top of.
    pub base: Hash,
    pub actor: Actor,
    pub intent: String,
    pub ops: Vec<Op>,
    pub footprint: Footprint,
    pub created: u64,
    /// The version this one replaced, so every version of a change can be
    /// compared (`grove evolve`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Hash>,
    /// The actor's signature over everything else in the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<crate::trust::Signature>,
}

/// One check's result, kept with the landing it judged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub check: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// How a change arrived on main.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum LandMode {
    /// The first landing of a repository.
    Init,
    /// Nothing else landed since the change's base.
    Direct,
    /// Others landed since its base, touching separate parts of the graph.
    Together { with: Vec<ChangeId> },
    /// Others landed since its base and touched what it depends on, so it was
    /// replayed on top of them and checked again.
    Ordered { after: Vec<ChangeId> },
}

/// One step of main's history: the snapshot it produced and the change that
/// produced it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Landing {
    pub snapshot: Hash,
    pub parent: Option<Hash>,
    pub change: Option<Hash>,
    #[serde(flatten)]
    pub mode: LandMode,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    pub at: u64,
}

/// One declaration both sides changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictDecl {
    pub id: DeclId,
    pub name: String,
    pub base: Option<String>,
    pub main: Option<String>,
    pub mine: Option<String>,
}

/// A clash that could not be merged. It is kept as an object so the work is
/// never lost, and anyone, a person or an agent, can resolve it later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub change: ChangeId,
    pub against: Vec<ChangeId>,
    pub decls: Vec<ConflictDecl>,
    pub at: u64,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A fresh identifier: a prefix and twelve hex characters from a hash of the
/// clock, the process and a counter.
pub fn fresh_id(prefix: &str, salt: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seed = format!(
        "{nanos}:{}:{}:{salt}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    );
    format!("{prefix}{}", &Hash::of("id", seed.as_bytes()).hex()[..12])
}

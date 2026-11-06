//! The program graph in memory: declarations with their source, the edges
//! between them, and applying ops to it.
//!
//! Edges are derived from content every time, so they can never disagree
//! with it. A reference from Zero code is a `Code` edge; a name mentioned in
//! backticks in a doc or other file is a `Mention` edge, which is inferred
//! and weaker. (Exact edges from Zero's own resolver are planned.)

use crate::hash::{decl_hash, Hash};
use crate::model::{DeclId, DeclKind, Entry, Op, Snapshot};
use crate::source::{classify, is_zero_file, references, rename_refs};
use crate::store::Store;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub entry: Entry,
    pub source: String,
}

impl Node {
    pub fn is_code(&self) -> bool {
        !matches!(self.entry.kind, DeclKind::File | DeclKind::Binary)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// A reference in Zero code.
    Code,
    /// A name in backticks in a non-code file: inferred.
    Mention,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: DeclId,
    pub to: DeclId,
    pub kind: EdgeKind,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ApplyError {
    #[error("{name} changed underneath this edit")]
    Stale { id: DeclId, name: String },
    #[error("declaration {id} is gone")]
    Missing { id: DeclId },
    #[error("declaration {id} already exists")]
    Exists { id: DeclId },
}

/// A program graph with every declaration's source loaded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub decls: BTreeMap<DeclId, Node>,
}

/// A rename that already landed, applied to work that started before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandedRename {
    pub from: String,
    pub to: String,
}

impl View {
    pub fn load(store: &Store, snap: &Snapshot) -> Result<View> {
        let mut decls = BTreeMap::new();
        for (id, entry) in &snap.decls {
            decls.insert(
                id.clone(),
                Node {
                    entry: entry.clone(),
                    source: store.get_decl(&entry.content)?,
                },
            );
        }
        Ok(View { decls })
    }

    /// Store every declaration's source and return the snapshot.
    pub fn save(&self, store: &Store) -> Result<Snapshot> {
        let mut decls = BTreeMap::new();
        for (id, node) in &self.decls {
            let content = store.put_decl(&node.source)?;
            decls.insert(
                id.clone(),
                Entry {
                    content,
                    ..node.entry.clone()
                },
            );
        }
        Ok(Snapshot { decls })
    }

    /// The snapshot this view would store, without storing it.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            decls: self
                .decls
                .iter()
                .map(|(id, n)| {
                    (
                        id.clone(),
                        Entry {
                            content: decl_hash(&n.source),
                            ..n.entry.clone()
                        },
                    )
                })
                .collect(),
        }
    }

    /// Referable names to ids. If two declarations share a name, the first by
    /// id wins and the clash is reported by `name_clashes`.
    pub fn names(&self) -> BTreeMap<String, DeclId> {
        let mut m = BTreeMap::new();
        for (id, n) in &self.decls {
            if n.entry.kind.is_referable() {
                m.entry(n.entry.name.clone()).or_insert_with(|| id.clone());
            }
        }
        m
    }

    pub fn name_clashes(&self) -> Vec<String> {
        let mut seen = BTreeMap::<&str, usize>::new();
        for n in self.decls.values().filter(|n| n.entry.kind.is_referable()) {
            *seen.entry(n.entry.name.as_str()).or_default() += 1;
        }
        seen.into_iter()
            .filter(|(_, c)| *c > 1)
            .map(|(n, _)| n.to_string())
            .collect()
    }

    /// Find a declaration by name (referable kinds first, then any).
    pub fn find(&self, name: &str) -> Option<DeclId> {
        self.names().get(name).cloned().or_else(|| {
            self.decls
                .iter()
                .find(|(_, n)| n.entry.name == name)
                .map(|(id, _)| id.clone())
        })
    }

    pub fn edges(&self) -> Vec<Edge> {
        // Zero's resolver, when it can answer, gives the exact code edges;
        // names still give the mentions from docs and other files.
        let resolved = crate::zero::resolved_edges(self);
        let names = self.names();
        let mut out: BTreeSet<Edge> = resolved.iter().flatten().cloned().collect();
        for (id, n) in &self.decls {
            let code = n.is_code();
            if code && resolved.is_some() {
                continue;
            }
            for r in references(&n.source, code) {
                if let Some(to) = names.get(&r) {
                    if to != id {
                        out.insert(Edge {
                            from: id.clone(),
                            to: to.clone(),
                            kind: if code {
                                EdgeKind::Code
                            } else {
                                EdgeKind::Mention
                            },
                        });
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    /// Declarations one edge away from any of `ids`, in either direction.
    pub fn neighbours(&self, ids: &BTreeSet<DeclId>) -> BTreeSet<DeclId> {
        let mut out = BTreeSet::new();
        for e in self.edges() {
            if ids.contains(&e.from) {
                out.insert(e.to.clone());
            }
            if ids.contains(&e.to) {
                out.insert(e.from.clone());
            }
        }
        out
    }

    /// Every file, materialized: Zero files as their declarations in order,
    /// one blank line apart; other files as they are.
    pub fn files(&self) -> BTreeMap<String, String> {
        let mut by_file: BTreeMap<&str, Vec<(&u64, &DeclId, &Node)>> = BTreeMap::new();
        for (id, n) in &self.decls {
            by_file
                .entry(n.entry.file.as_str())
                .or_default()
                .push((&n.entry.order, id, n));
        }
        by_file
            .into_iter()
            .map(|(file, mut items)| {
                items.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
                let text = if is_zero_file(file) {
                    items
                        .iter()
                        .map(|(_, _, n)| n.source.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    items
                        .iter()
                        .map(|(_, _, n)| n.source.as_str())
                        .collect::<String>()
                };
                (file.to_string(), text)
            })
            .collect()
    }

    /// Every file's bytes, as they go on disk: binary files decoded.
    pub fn file_bytes(&self) -> BTreeMap<String, Vec<u8>> {
        let binary: BTreeSet<&str> = self
            .decls
            .values()
            .filter(|n| n.entry.kind == DeclKind::Binary)
            .map(|n| n.entry.file.as_str())
            .collect();
        self.files()
            .into_iter()
            .map(|(f, text)| {
                let bytes = if binary.contains(f.as_str()) {
                    crate::source::decode_binary(&text)
                } else {
                    text.into_bytes()
                };
                (f, bytes)
            })
            .collect()
    }

    /// Apply ops in order. `landed` are renames that reached main after the
    /// ops were written: they are applied to the incoming source too, so an
    /// edit written against the old name still lands correctly. Returns the
    /// declarations whose content changed.
    pub fn apply(
        &mut self,
        store: &Store,
        ops: &[Op],
        landed: &[LandedRename],
    ) -> Result<BTreeSet<DeclId>, ApplyError> {
        let mut touched = BTreeSet::new();
        let carry = |source: &str, code: bool| -> String {
            landed.iter().fold(source.to_string(), |s, r| {
                rename_refs(&s, &r.from, &r.to, code)
            })
        };
        // Does the current content match what the op expected, allowing for
        // renames that landed since?
        let matches = |node: &Node, expect: &Hash| -> bool {
            let current = decl_hash(&node.source);
            if &current == expect {
                return true;
            }
            match store.get_decl(expect) {
                Ok(old) => decl_hash(&carry(&old, node.is_code())) == current,
                Err(_) => false,
            }
        };
        for op in ops {
            match op {
                Op::Add {
                    id,
                    name,
                    kind,
                    file,
                    order,
                    source,
                } => {
                    if self.decls.contains_key(id) {
                        return Err(ApplyError::Exists { id: id.clone() });
                    }
                    let code = !matches!(*kind, DeclKind::File | DeclKind::Binary);
                    let source = carry(source, code);
                    let name = if code {
                        classify(&source).0
                    } else {
                        name.clone()
                    };
                    self.decls.insert(
                        id.clone(),
                        Node {
                            entry: Entry {
                                name,
                                kind: *kind,
                                file: file.clone(),
                                order: *order,
                                content: decl_hash(&source),
                            },
                            source,
                        },
                    );
                    touched.insert(id.clone());
                }
                Op::Replace { id, expect, source } => {
                    let node = self
                        .decls
                        .get(id)
                        .ok_or_else(|| ApplyError::Missing { id: id.clone() })?;
                    if !matches(node, expect) {
                        return Err(ApplyError::Stale {
                            id: id.clone(),
                            name: node.entry.name.clone(),
                        });
                    }
                    let code = node.is_code();
                    let source = carry(source, code);
                    let node = self.decls.get_mut(id).unwrap();
                    if code {
                        node.entry.name = classify(&source).0;
                    }
                    node.entry.content = decl_hash(&source);
                    node.source = source;
                    touched.insert(id.clone());
                }
                Op::Rename { id, from, to } => {
                    let node = self
                        .decls
                        .get(id)
                        .ok_or_else(|| ApplyError::Missing { id: id.clone() })?;
                    if node.entry.name != *from {
                        if node.entry.name == *to {
                            continue;
                        }
                        return Err(ApplyError::Stale {
                            id: id.clone(),
                            name: node.entry.name.clone(),
                        });
                    }
                    for (did, n) in self.decls.iter_mut() {
                        let code = n.is_code();
                        let next = rename_refs(&n.source, from, to, code);
                        // the declaration's own header names it in code
                        let next = if did == id && code {
                            rename_header(&next, from, to)
                        } else {
                            next
                        };
                        if next != n.source {
                            n.entry.content = decl_hash(&next);
                            n.source = next;
                            touched.insert(did.clone());
                        }
                    }
                    let node = self.decls.get_mut(id).unwrap();
                    node.entry.name = to.clone();
                    touched.insert(id.clone());
                }
                Op::Delete { id, expect } => {
                    let node = self
                        .decls
                        .get(id)
                        .ok_or_else(|| ApplyError::Missing { id: id.clone() })?;
                    if !matches(node, expect) {
                        return Err(ApplyError::Stale {
                            id: id.clone(),
                            name: node.entry.name.clone(),
                        });
                    }
                    self.decls.remove(id);
                    touched.insert(id.clone());
                }
            }
        }
        Ok(touched)
    }
}

/// `rename_refs` already rewrote the header unless the name was preceded by
/// a dot; this covers a declaration renamed when nothing else refers to it.
fn rename_header(source: &str, from: &str, to: &str) -> String {
    rename_refs(source, from, to, true)
}

/// Is this path a Zero file?
pub fn zero_file(path: &str) -> bool {
    is_zero_file(path)
}

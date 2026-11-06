//! Turning edited files into ops.
//!
//! People and agents edit files as usual. Grove compares the working files
//! with the change's base, declaration by declaration, and writes ops:
//! renames first (detected when a declaration reappears under a new name
//! with every reference to it updated), then replaced bodies, deletions and
//! additions. A call site that differs only by the rename gets no op of its
//! own: the rename carries it, which is what lets it land on top of other
//! people's work.

use crate::graph::View;
use crate::hash::decl_hash;
use crate::hash::Hash;
use crate::model::{DeclId, DeclKind, Op};
use crate::source::{rename_refs, Item};
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

/// The working copy, file by file, each file's declarations in order.
pub type Work = BTreeMap<String, Vec<Item>>;

/// A new declaration's id, derived from where it lives and what it's called,
/// never from the machine or the clock. The same code gets the same ids on
/// every machine, and two people adding the same declaration at once get the
/// same id, so Grove sees a real conflict instead of two silent copies. If
/// the id is taken (an old declaration was renamed away from this name), a
/// counter is added, also deterministically.
pub fn decl_id(file: &str, kind: DeclKind, name: &str, taken: &BTreeSet<DeclId>) -> DeclId {
    let prefix = if matches!(kind, DeclKind::File | DeclKind::Binary) {
        "f_"
    } else {
        "d_"
    };
    let kind = serde_json::to_string(&kind).unwrap_or_default();
    (0u32..)
        .map(|n| {
            format!(
                "{prefix}{}",
                &Hash::of("declid", format!("{file}\0{kind}\0{name}\0{n}").as_bytes()).hex()[..16]
            )
        })
        .find(|id| !taken.contains(id))
        .expect("an unused id")
}

#[allow(clippy::type_complexity)]
pub fn diff(store: &Store, base: &View, work: &Work) -> Vec<Op> {
    // index the base by (file, name)
    let mut base_keys: BTreeMap<(String, String), DeclId> = BTreeMap::new();
    for (id, n) in &base.decls {
        base_keys.insert((n.entry.file.clone(), n.entry.name.clone()), id.clone());
    }
    let mut work_keys: BTreeMap<(String, String), (u64, &Item)> = BTreeMap::new();
    for (file, items) in work {
        for (i, item) in items.iter().enumerate() {
            work_keys.insert((file.clone(), item.name.clone()), (i as u64, item));
        }
    }

    let removed: Vec<&DeclId> = base_keys
        .iter()
        .filter(|(k, _)| !work_keys.contains_key(*k))
        .map(|(_, id)| id)
        .collect();
    let added: Vec<(&(String, String), &(u64, &Item))> = work_keys
        .iter()
        .filter(|(k, _)| !base_keys.contains_key(*k))
        .collect();

    // renames: a removed declaration that reappears under a new name, same
    // file and kind, with nothing else changed
    let mut ops = vec![];
    let mut renamed_ids = BTreeSet::new();
    let mut renamed_keys = BTreeSet::new();
    for (key, (_, item)) in &added {
        if !item.kind.is_referable() {
            continue;
        }
        for rid in &removed {
            if renamed_ids.contains(*rid) {
                continue;
            }
            let r = &base.decls[*rid];
            if r.entry.file == key.0
                && r.entry.kind == item.kind
                && rename_refs(&r.source, &r.entry.name, &item.name, true) == item.source
            {
                ops.push(Op::Rename {
                    id: (*rid).clone(),
                    from: r.entry.name.clone(),
                    to: item.name.clone(),
                });
                renamed_ids.insert((*rid).clone());
                renamed_keys.insert((*key).clone());
                break;
            }
        }
    }

    // what the base looks like once the renames are applied
    let mut after_renames = base.clone();
    after_renames
        .apply(store, &ops, &[])
        .expect("renames apply cleanly to their own base");

    // replaced bodies, judged against the renamed base
    for (key, (_, item)) in &work_keys {
        let id = match base_keys.get(key) {
            Some(id) => id.clone(),
            None if renamed_keys.contains(key) => match ops.iter().find_map(|op| match op {
                Op::Rename { id, to, .. } if *to == key.1 && base.decls[id].entry.file == key.0 => {
                    Some(id.clone())
                }
                _ => None,
            }) {
                Some(id) => id,
                None => continue,
            },
            None => continue,
        };
        let now = &after_renames.decls[&id];
        if now.source != item.source {
            ops.push(Op::Replace {
                id,
                expect: decl_hash(&now.source),
                source: item.source.clone(),
            });
        }
    }

    for rid in removed {
        if !renamed_ids.contains(rid) {
            let now = &after_renames.decls[rid];
            ops.push(Op::Delete {
                id: rid.clone(),
                expect: decl_hash(&now.source),
            });
        }
    }

    let mut taken: BTreeSet<DeclId> = base.decls.keys().cloned().collect();
    for (key, (order, item)) in added {
        if renamed_keys.contains(key) {
            continue;
        }
        let id = decl_id(&key.0, item.kind, &key.1, &taken);
        taken.insert(id.clone());
        ops.push(Op::Add {
            id,
            name: item.name.clone(),
            kind: item.kind,
            file: key.0.clone(),
            order: *order,
            source: item.source.clone(),
        });
    }
    ops
}

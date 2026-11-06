//! Working together: approvals, remembered resolutions, notes and presence.

use crate::hash::Hash;
use crate::model::*;
use crate::repo::{short, Repo};
use crate::trust::{can_approve, Approval};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A note on a change (git's notes).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub by: Actor,
    pub text: String,
    pub at: u64,
}

/// Who is working on what right now, for one workspace.
#[derive(Clone, Debug)]
pub struct Presence {
    pub workspace: String,
    pub actor: Option<Actor>,
    pub lane: Option<String>,
    /// The change recorded and not yet landed, and the declarations it writes.
    pub draft: Option<(Change, Vec<String>)>,
    /// Edits not recorded yet.
    pub unrecorded: usize,
}

fn resolution_key(base: &str, a: &str, b: &str) -> String {
    Hash::of("resolution", format!("{base}\0{a}\0{b}").as_bytes())
        .hex()
        .to_string()
}

impl Repo {
    fn resolutions_path(&self) -> std::path::PathBuf {
        self.dir.join("resolutions.json")
    }

    fn resolutions(&self) -> BTreeMap<String, Hash> {
        std::fs::read(self.resolutions_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// A resolution remembered for this exact clash (git's rerere).
    pub fn resolution(&self, base: &str, main: &str, mine: &str) -> Option<String> {
        let map = self.resolutions();
        let h = map
            .get(&resolution_key(base, main, mine))
            .or_else(|| map.get(&resolution_key(base, mine, main)))?;
        self.store.get_decl(h).ok()
    }

    /// When a conflict is closed, remember how main now reads for each
    /// declaration it was about, so the same clash resolves itself next time.
    pub fn remember_resolution(&self, conflict: &Conflict) -> Result<usize> {
        let main = self.main_view()?;
        let mut map = self.resolutions();
        let mut n = 0;
        for d in &conflict.decls {
            if let (Some(b), Some(m), Some(y), Some(now)) =
                (&d.base, &d.main, &d.mine, main.decls.get(&d.id))
            {
                if now.source != *m || now.source == *y {
                    let h = self.store.put_decl(&now.source)?;
                    map.insert(resolution_key(b, m, y), h);
                    n += 1;
                }
            }
        }
        crate::store::write_atomic(&self.resolutions_path(), &serde_json::to_vec_pretty(&map)?)?;
        Ok(n)
    }

    /// A person approves the current version of a draft.
    pub fn approve(&self, prefix: &str, by: &Actor) -> Result<Change> {
        let change = self.find_change(prefix)?;
        can_approve(by, &change)?;
        self.transact("approve", &by.to_string(), |refs| {
            if !refs.drafts.contains_key(&change.id) {
                return Err(anyhow!("{} already landed", short(&change.id)));
            }
            let list = refs.approvals.entry(change.id.clone()).or_default();
            if !list
                .iter()
                .any(|a| a.by == *by && a.version == change.version)
            {
                let mut a = Approval {
                    by: by.clone(),
                    version: change.version,
                    at: now(),
                    signature: None,
                };
                crate::trust::sign_approval(&change.id, &mut a);
                list.push(a);
            }
            Ok((
                change.clone(),
                format!(
                    "{} approved {} v{}",
                    by.name,
                    short(&change.id),
                    change.version
                ),
            ))
        })
    }

    /// Add a note to a change.
    pub fn note(&self, prefix: &str, text: &str, by: &Actor) -> Result<()> {
        let change = self.find_any_change(prefix)?;
        let h = self.store.put_json(
            "note",
            &Note {
                by: by.clone(),
                text: text.into(),
                at: now(),
            },
        )?;
        self.transact("note", &by.to_string(), |refs| {
            refs.notes
                .entry(change.id.clone())
                .or_default()
                .push(h.clone());
            Ok(((), format!("noted {}", short(&change.id))))
        })
    }

    pub fn notes(&self, change_id: &str) -> Result<Vec<Note>> {
        let refs = self.refs()?;
        refs.notes
            .get(change_id)
            .map(|v| v.iter().map(|h| self.store.get_json("note", h)).collect())
            .unwrap_or_else(|| Ok(vec![]))
    }

    /// Who is working where: every workspace's owner, lane, draft and
    /// unrecorded edits.
    pub fn who(&self) -> Result<Vec<Presence>> {
        let refs = self.refs()?;
        let mut out = vec![];
        for (name, w) in &refs.workspaces {
            let draft = match w.draft.as_ref().and_then(|d| refs.drafts.get(d)) {
                Some(h) => {
                    let c = self.change(h)?;
                    let view = self.view_at(&c.base)?;
                    let names = c
                        .footprint
                        .writes
                        .iter()
                        .map(|id| {
                            view.decls
                                .get(id)
                                .map(|n| n.entry.name.clone())
                                .unwrap_or_else(|| {
                                    c.ops
                                        .iter()
                                        .find_map(|op| match op {
                                            Op::Add { id: a, name, .. } if a == id => {
                                                Some(name.clone())
                                            }
                                            _ => None,
                                        })
                                        .unwrap_or_else(|| id.clone())
                                })
                        })
                        .collect();
                    Some((c, names))
                }
                None => None,
            };
            let unrecorded = if std::path::Path::new(&w.path).exists() {
                self.status(name).map(|o| o.len()).unwrap_or(0)
            } else {
                0
            };
            out.push(Presence {
                workspace: name.clone(),
                actor: w.actor.clone(),
                lane: w.lane.clone(),
                draft,
                unrecorded,
            });
        }
        Ok(out)
    }
}

impl Repo {
    /// Change the repository's config (checks, review rules, trusted keys).
    pub fn update_config(
        &self,
        f: impl FnOnce(&mut crate::repo::Config) -> Result<()>,
    ) -> Result<crate::repo::Config> {
        let mut c = self.config()?;
        f(&mut c)?;
        crate::store::write_atomic(
            &self.dir.join("config.json"),
            &serde_json::to_vec_pretty(&c)?,
        )?;
        Ok(c)
    }
}

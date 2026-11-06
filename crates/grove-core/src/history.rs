//! Working with history: revert, pick, restore, shelve, tags, lanes, find,
//! evolve, shortlog, bisect and text diffs. Each of git's history tools has
//! its Grove form here, built on the same landing machinery so the rules
//! (replay by meaning, footprints, checks, undo) hold for all of them.

use crate::diff::{diff, Work};
use crate::graph::View;
use crate::hash::{decl_hash, Hash};
use crate::model::*;
use crate::repo::{
    footprint, head, set_head, short, work_files, LandOutcome, Repo, DEFAULT_WORKSPACE,
};
use crate::source::Item;
use crate::textdiff::unified;
use crate::workdir::materialize;
use anyhow::{anyhow, bail, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Where `grove bisect` found the first failing landing.
#[derive(Clone, Debug)]
pub struct Bisect {
    pub first_bad: Hash,
    pub change: Option<Change>,
    pub tested: usize,
}

/// A view as a working copy would read it.
pub fn view_to_work(view: &View) -> Work {
    let mut by_file: BTreeMap<String, Vec<(u64, &DeclId, Item)>> = BTreeMap::new();
    for (id, n) in &view.decls {
        by_file.entry(n.entry.file.clone()).or_default().push((
            n.entry.order,
            id,
            Item {
                name: n.entry.name.clone(),
                kind: n.entry.kind,
                source: n.source.clone(),
            },
        ));
    }
    by_file
        .into_iter()
        .map(|(f, mut items)| {
            items.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
            (f, items.into_iter().map(|(_, _, i)| i).collect())
        })
        .collect()
}

fn shell(cmd: &str, dir: &Path) -> bool {
    crate::shell::run(cmd, dir)
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl Repo {
    /// Store a change as a draft, outside any workspace.
    pub fn stage(&self, change: &Change, actor: &Actor) -> Result<Hash> {
        self.transact("stage", &actor.to_string(), |refs| {
            let h = self.store.put_json("change", change)?;
            refs.drafts.insert(change.id.clone(), h.clone());
            Ok((
                h,
                format!("staged {} \"{}\"", short(&change.id), change.intent),
            ))
        })
    }

    /// The landing that brought a change in, searching back from `from`.
    pub fn landing_of(&self, from: &Hash, prefix: &str) -> Result<(Hash, Landing, Change)> {
        for (h, l) in self.history(from, None)? {
            if let Some(ch) = &l.change {
                let c = self.change(ch)?;
                if c.id.starts_with(prefix) || short(&c.id).starts_with(prefix) {
                    return Ok((h, l, c));
                }
            }
        }
        bail!("no landed change matching {prefix}")
    }

    /// The newest landing both histories share.
    pub fn merge_base(&self, a: &Hash, b: &Hash) -> Result<Hash> {
        let seen: BTreeSet<Hash> = self.history(a, None)?.into_iter().map(|(h, _)| h).collect();
        for (h, _) in self.history(b, None)? {
            if seen.contains(&h) {
                return Ok(h);
            }
        }
        bail!("these two lines of work share no history")
    }

    fn lane_of(&self, ws: &str) -> Result<Option<String>> {
        Ok(self.refs()?.workspaces.get(ws).and_then(|w| w.lane.clone()))
    }

    // --------------------------------------------------------- revert, pick

    /// Undo what a landed change did, as a new change that lands like any
    /// other: replayed, ordered, checked, and undoable itself.
    pub fn revert(&self, ws: &str, prefix: &str, actor: &Actor) -> Result<LandOutcome> {
        let refs = self.refs()?;
        let lane = self.lane_of(ws)?;
        let top = head(&refs, lane.as_deref())?;
        let (lh, l, c) = self.landing_of(&top, prefix)?;
        let parent = l
            .parent
            .clone()
            .ok_or_else(|| anyhow!("the first landing can't be reverted"))?;
        let before = self.view_at(&parent)?;
        let after = self.view_at(&lh)?;

        // renames go back first, so the other ops see the original names
        let renames: Vec<Op> = c
            .ops
            .iter()
            .rev()
            .filter_map(|op| match op {
                Op::Rename { id, from, to } => Some(Op::Rename {
                    id: id.clone(),
                    from: to.clone(),
                    to: from.clone(),
                }),
                _ => None,
            })
            .collect();
        let mut mid = after.clone();
        mid.apply(&self.store, &renames, &[])
            .map_err(|e| anyhow!(e))?;
        let mut ops = renames;
        for op in c.ops.iter().rev() {
            match op {
                Op::Add { id, .. } => {
                    if let Some(n) = mid.decls.get(id) {
                        ops.push(Op::Delete {
                            id: id.clone(),
                            expect: decl_hash(&n.source),
                        });
                    }
                }
                Op::Replace { id, .. } => {
                    if let (Some(b), Some(a)) = (before.decls.get(id), mid.decls.get(id)) {
                        if a.source != b.source {
                            ops.push(Op::Replace {
                                id: id.clone(),
                                expect: decl_hash(&a.source),
                                source: b.source.clone(),
                            });
                        }
                    }
                }
                Op::Delete { id, .. } => {
                    if let Some(b) = before.decls.get(id) {
                        ops.push(Op::Add {
                            id: id.clone(),
                            name: b.entry.name.clone(),
                            kind: b.entry.kind,
                            file: b.entry.file.clone(),
                            order: b.entry.order,
                            source: b.source.clone(),
                        });
                    }
                }
                Op::Rename { .. } => {}
            }
        }
        let mut result = after.clone();
        let touched = result
            .apply(&self.store, &ops, &[])
            .map_err(|e| anyhow!(e))?;
        let change = Change {
            id: fresh_id("c_", &format!("revert {}", c.id)),
            version: 1,
            base: lh,
            actor: actor.clone(),
            intent: format!("Revert \"{}\"", c.intent),
            footprint: footprint(&after, &result, &ops, &touched),
            ops,
            created: now(),
            previous: None,
        };
        self.stage(&change, actor)?;
        self.land(ws, Some(&change.id), actor)
    }

    /// Find a change anywhere: drafts, main, lanes, shelves.
    pub fn find_any_change(&self, prefix: &str) -> Result<Change> {
        if let Ok(c) = self.find_change(prefix) {
            return Ok(c);
        }
        let refs = self.refs()?;
        for h in refs.lanes.values() {
            if let Ok((_, _, c)) = self.landing_of(h, prefix) {
                return Ok(c);
            }
        }
        for h in refs.shelves.values() {
            let c = self.change(h)?;
            if c.id.starts_with(prefix) || short(&c.id).starts_with(prefix) {
                return Ok(c);
            }
        }
        bail!("no change matching {prefix}")
    }

    /// Apply a change from anywhere (a lane, a revert, an undone landing) on
    /// top of this workspace's line of work, as a new change.
    pub fn pick(&self, ws: &str, prefix: &str, actor: &Actor) -> Result<LandOutcome> {
        let c = self.find_any_change(prefix)?;
        let refs = self.refs()?;
        let lane = self.lane_of(ws)?;
        let top = head(&refs, lane.as_deref())?;
        let on_line: BTreeSet<Hash> = self
            .history(&top, None)?
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        let base = if on_line.contains(&c.base) {
            c.base.clone()
        } else {
            self.merge_base(&c.base, &top)?
        };
        let change = Change {
            id: fresh_id("c_", &format!("pick {}", c.id)),
            version: 1,
            base,
            actor: actor.clone(),
            intent: format!("{} (picked from {})", c.intent, short(&c.id)),
            ops: c.ops.clone(),
            footprint: c.footprint.clone(),
            created: now(),
            previous: None,
        };
        self.stage(&change, actor)?;
        self.land(ws, Some(&change.id), actor)
    }

    // ------------------------------------------------------ restore, shelve

    /// Throw away a workspace's unrecorded edits: to the given files, or all.
    pub fn restore(&self, ws: &str, paths: &[String]) -> Result<Vec<String>> {
        let refs = self.refs()?;
        let w = refs
            .workspaces
            .get(ws)
            .ok_or_else(|| anyhow!("no workspace named {ws}"))?;
        let base = self.view_at(&w.base)?;
        let root = Path::new(&w.path);
        let base_files = base.file_bytes();
        let work = self.read_workspace(&refs, ws)?;
        let current = work_files(&work);
        let wanted: Vec<String> = if paths.is_empty() {
            base_files
                .keys()
                .chain(current.keys())
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        } else {
            paths.iter().map(|p| p.replace('\\', "/")).collect()
        };
        let mut restored = vec![];
        for p in wanted {
            let target = root.join(&p);
            match base_files.get(&p) {
                Some(bytes) if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) => {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&target, bytes)?;
                    restored.push(p);
                }
                Some(_) => {}
                None if current.contains_key(&p) => {
                    std::fs::remove_file(&target)?;
                    restored.push(p);
                }
                None => {}
            }
        }
        Ok(restored)
    }

    /// Put a workspace's unrecorded edits aside under a name, and restore it.
    pub fn shelve(&self, ws: &str, name: &str, actor: &Actor) -> Result<usize> {
        let ops = self.status(ws)?;
        if ops.is_empty() {
            bail!("nothing to shelve: the workspace matches its base");
        }
        let n = ops.len();
        self.transact("shelve", &actor.to_string(), |refs| {
            if refs.shelves.contains_key(name) {
                bail!("there is already a shelf named {name}");
            }
            let w = refs
                .workspaces
                .get(ws)
                .ok_or_else(|| anyhow!("no workspace named {ws}"))?
                .clone();
            let base = self.view_at(&w.base)?;
            let mut after = base.clone();
            let touched = after
                .apply(&self.store, &ops, &[])
                .map_err(|e| anyhow!(e))?;
            let change = Change {
                id: fresh_id("c_", name),
                version: 1,
                base: w.base.clone(),
                actor: actor.clone(),
                intent: format!("Shelved: {name}"),
                footprint: footprint(&base, &after, &ops, &touched),
                ops: ops.clone(),
                created: now(),
                previous: None,
            };
            refs.shelves
                .insert(name.into(), self.store.put_json("change", &change)?);
            Ok(((), format!("shelved {n} edit(s) as {name}")))
        })?;
        self.restore(ws, &[])?;
        Ok(n)
    }

    /// Bring shelved edits back into a workspace, on top of what it has now.
    pub fn unshelve(&self, ws: &str, name: &str, actor: &Actor) -> Result<usize> {
        self.transact("unshelve", &actor.to_string(), |refs| {
            let h = refs
                .shelves
                .remove(name)
                .ok_or_else(|| anyhow!("no shelf named {name}"))?;
            let c = self.change(&h)?;
            let w = refs
                .workspaces
                .get(ws)
                .ok_or_else(|| anyhow!("no workspace named {ws}"))?
                .clone();
            let base = self.view_at(&w.base)?;
            let local = diff(&self.store, &base, &self.read_workspace(refs, ws)?);
            let mut current = base.clone();
            current
                .apply(&self.store, &local, &[])
                .map_err(|e| anyhow!(e))?;
            let renames = if self
                .history(&w.base, None)?
                .iter()
                .any(|(h, _)| *h == c.base)
            {
                crate::repo::renames_of(&self.landed_since(&w.base, &c.base)?)
            } else {
                vec![]
            };
            let mut result = current.clone();
            result
                .apply(&self.store, &c.ops, &renames)
                .map_err(|e| anyhow!("the shelf clashes with this workspace: {e}"))?;
            materialize(Path::new(&w.path), &result, Some(&current))?;
            Ok((c.ops.len(), format!("unshelved {name}")))
        })
    }

    // ----------------------------------------------------------------- tags

    /// Resolve a landing by tag, landing hash prefix, or the change it landed.
    pub fn resolve_landing(&self, what: &str) -> Result<Hash> {
        let refs = self.refs()?;
        if let Some(h) = refs.tags.get(what).or_else(|| refs.lanes.get(what)) {
            return Ok(h.clone());
        }
        if what == "main" {
            return self.main();
        }
        for (h, l) in self.history(&self.main()?, None)? {
            if h.hex().starts_with(what) {
                return Ok(h);
            }
            if let Some(ch) = &l.change {
                let c = self.change(ch)?;
                if c.id.starts_with(what) || short(&c.id).starts_with(what) {
                    return Ok(h);
                }
            }
        }
        bail!("no tag, lane, landing or change matching {what}")
    }

    pub fn tag(&self, name: &str, at: Option<&str>, actor: &Actor) -> Result<Hash> {
        let target = match at {
            Some(a) => self.resolve_landing(a)?,
            None => self.main()?,
        };
        self.transact("tag", &actor.to_string(), |refs| {
            if refs.tags.contains_key(name) {
                bail!("tag {name} already exists");
            }
            refs.tags.insert(name.into(), target.clone());
            Ok((
                target.clone(),
                format!("tagged {} as {name}", target.short()),
            ))
        })
    }

    pub fn untag(&self, name: &str, actor: &Actor) -> Result<()> {
        self.transact("tag", &actor.to_string(), |refs| {
            refs.tags
                .remove(name)
                .ok_or_else(|| anyhow!("no tag named {name}"))?;
            Ok(((), format!("removed tag {name}")))
        })
    }

    // ---------------------------------------------------------------- lanes

    /// Start a named line of work beside main.
    pub fn lane_new(&self, name: &str, from: Option<&str>, actor: &Actor) -> Result<Hash> {
        let start = match from {
            Some(f) => self.resolve_landing(f)?,
            None => self.main()?,
        };
        self.transact("lane", &actor.to_string(), |refs| {
            if name == "main" || refs.lanes.contains_key(name) {
                bail!("lane {name} already exists");
            }
            refs.lanes.insert(name.into(), start.clone());
            Ok((start.clone(), format!("started lane {name}")))
        })
    }

    /// Point a workspace at a lane (or back at main with `None`). The
    /// workspace must have no unrecorded edits: record or shelve them first.
    pub fn lane_use(&self, ws: &str, lane: Option<&str>, actor: &Actor) -> Result<()> {
        if !self.status(ws)?.is_empty() {
            bail!("{ws} has unrecorded edits: record them (`grove new`) or shelve them (`grove shelve`) first");
        }
        self.transact("lane", &actor.to_string(), |refs| {
            let new_head = head(refs, lane)?;
            let w = refs
                .workspaces
                .get(ws)
                .ok_or_else(|| anyhow!("no workspace named {ws}"))?
                .clone();
            let old = self.view_at(&w.base)?;
            materialize(Path::new(&w.path), &self.view_at(&new_head)?, Some(&old))?;
            let wm = refs.workspaces.get_mut(ws).unwrap();
            wm.lane = lane.map(str::to_string);
            wm.base = new_head;
            Ok(((), format!("{ws} now works on {}", lane.unwrap_or("main"))))
        })
    }

    /// Land everything a lane did onto main as one change, replayed and
    /// checked like any other.
    pub fn lane_land(&self, name: &str, actor: &Actor) -> Result<LandOutcome> {
        let refs = self.refs()?;
        let top = refs
            .lanes
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("no lane named {name}"))?;
        let main = self.main()?;
        let fork = self.merge_base(&top, &main)?;
        if fork == top {
            bail!("lane {name} has nothing main doesn't already have");
        }
        let n = self.history(&top, Some(&fork))?.len();
        let fork_view = self.view_at(&fork)?;
        let lane_view = self.view_at(&top)?;
        let ops = diff(&self.store, &fork_view, &view_to_work(&lane_view));
        let mut after = fork_view.clone();
        let touched = after
            .apply(&self.store, &ops, &[])
            .map_err(|e| anyhow!(e))?;
        let change = Change {
            id: fresh_id("c_", &format!("lane {name}")),
            version: 1,
            base: fork,
            actor: actor.clone(),
            intent: format!(
                "Land lane {name} ({n} change{})",
                if n == 1 { "" } else { "s" }
            ),
            footprint: footprint(&fork_view, &after, &ops, &touched),
            ops,
            created: now(),
            previous: None,
        };
        self.stage(&change, actor)?;
        self.land_on(None, DEFAULT_WORKSPACE, Some(&change.id), actor)
    }

    pub fn lane_drop(&self, name: &str, actor: &Actor) -> Result<()> {
        self.transact("lane", &actor.to_string(), |refs| {
            if refs
                .workspaces
                .values()
                .any(|w| w.lane.as_deref() == Some(name))
            {
                bail!("a workspace still works on lane {name}; point it elsewhere first");
            }
            refs.lanes
                .remove(name)
                .ok_or_else(|| anyhow!("no lane named {name}"))?;
            Ok(((), format!("dropped lane {name}")))
        })
    }

    // ------------------------------------------------------------- looking

    /// Lines on main containing `query`, as (file, line number, line).
    pub fn find(&self, query: &str) -> Result<Vec<(String, usize, String)>> {
        let view = self.main_view()?;
        let binary: BTreeSet<String> = view
            .decls
            .values()
            .filter(|n| n.entry.kind == DeclKind::Binary)
            .map(|n| n.entry.file.clone())
            .collect();
        let mut out = vec![];
        for (file, text) in view.files() {
            if binary.contains(&file) {
                continue;
            }
            for (i, line) in text.lines().enumerate() {
                if line.contains(query) {
                    out.push((file.clone(), i + 1, line.to_string()));
                }
            }
        }
        Ok(out)
    }

    /// Every version of a change, newest first.
    pub fn evolve(&self, prefix: &str) -> Result<Vec<Change>> {
        let mut c = self.find_any_change(prefix)?;
        let mut out = vec![c.clone()];
        while let Some(p) = c.previous.clone() {
            c = self.change(&p)?;
            out.push(c.clone());
        }
        Ok(out)
    }

    /// Landed changes per actor on main.
    pub fn shortlog(&self) -> Result<Vec<(String, usize)>> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for (_, l) in self.history(&self.main()?, None)? {
            if let Some(ch) = &l.change {
                *counts
                    .entry(self.change(ch)?.actor.to_string())
                    .or_default() += 1;
            }
        }
        let mut v: Vec<_> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(v)
    }

    /// Find the first landing on main where `cmd` fails. `good` names a
    /// landing where it passed (default: the first landing).
    pub fn bisect(&self, cmd: &str, good: Option<&str>) -> Result<Bisect> {
        let main = self.main()?;
        let stop = match good {
            Some(g) => Some(self.resolve_landing(g)?),
            None => None,
        };
        let mut line: Vec<Hash> = self
            .history(&main, stop.as_ref())?
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        line.reverse(); // oldest first, all after `good`
        if stop.is_none() && !line.is_empty() {
            line.remove(0); // the first landing is assumed good
        }
        let mut tested = 0;
        let test = |h: &Hash, tested: &mut usize| -> Result<bool> {
            *tested += 1;
            let dir = std::env::temp_dir().join(fresh_id("grove-bisect-", h.as_str()));
            std::fs::create_dir_all(&dir)?;
            materialize(&dir, &self.view_at(h)?, None)?;
            let ok = shell(cmd, &dir);
            let _ = std::fs::remove_dir_all(&dir);
            Ok(ok)
        };
        if line.is_empty() || test(line.last().unwrap(), &mut tested)? {
            bail!("the command passes on main, so there is nothing to find");
        }
        let (mut lo, mut hi) = (0usize, line.len() - 1); // hi fails
        while lo < hi {
            let mid = (lo + hi) / 2;
            if test(&line[mid], &mut tested)? {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let first_bad = line[hi].clone();
        let change = match &self.landing(&first_bad)?.change {
            Some(c) => Some(self.change(c)?),
            None => None,
        };
        Ok(Bisect {
            first_bad,
            change,
            tested,
        })
    }

    /// A git-style text diff: a workspace's unrecorded edits, or a change.
    pub fn diff_text(&self, ws: &str, change: Option<&str>) -> Result<String> {
        let (before, after) = match change {
            Some(p) => {
                let c = self.find_any_change(p)?;
                let before = self.view_at(&c.base)?;
                let mut after = before.clone();
                after
                    .apply(&self.store, &c.ops, &[])
                    .map_err(|e| anyhow!(e))?;
                (before, after)
            }
            None => {
                let refs = self.refs()?;
                let w = refs
                    .workspaces
                    .get(ws)
                    .ok_or_else(|| anyhow!("no workspace named {ws}"))?;
                let before = self.view_at(&w.base)?;
                let mut after = before.clone();
                let ops = diff(&self.store, &before, &self.read_workspace(&refs, ws)?);
                after
                    .apply(&self.store, &ops, &[])
                    .map_err(|e| anyhow!(e))?;
                (before, after)
            }
        };
        let is_binary = |v: &View, f: &str| {
            v.decls
                .values()
                .any(|n| n.entry.file == f && n.entry.kind == DeclKind::Binary)
        };
        let (a, b) = (before.files(), after.files());
        let mut out = String::new();
        for f in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
            let (old, new) = (
                a.get(f).map(String::as_str).unwrap_or(""),
                b.get(f).map(String::as_str).unwrap_or(""),
            );
            if old == new {
                continue;
            }
            if is_binary(&before, f) || is_binary(&after, f) {
                out.push_str(&format!("Binary file {f} changed\n"));
                continue;
            }
            let (la, lb) = (
                if a.contains_key(f) {
                    format!("a/{f}")
                } else {
                    "/dev/null".into()
                },
                if b.contains_key(f) {
                    format!("b/{f}")
                } else {
                    "/dev/null".into()
                },
            );
            out.push_str(&unified(old, new, &la, &lb));
        }
        Ok(out)
    }

    /// Point main at a landing directly (used by imports that already decided
    /// history). Logged and undoable like everything else.
    pub fn set_main(&self, h: &Hash, actor: &Actor) -> Result<()> {
        self.transact("set-main", &actor.to_string(), |refs| {
            set_head(refs, None, h.clone());
            Ok(((), format!("main is {}", h.short())))
        })
    }
}

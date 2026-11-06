//! A Grove repository: refs, the operation log, workspaces, and the
//! commands that move work through them.
//!
//! Layout under `.grove/`:
//!
//! ```text
//! objects/     content-addressed objects (declarations, snapshots, changes, landings)
//! refs.json    main, drafts, workspaces, open conflicts (replaced atomically)
//! oplog.jsonl  every operation, with the refs before and after (for undo)
//! config.json  checks to run before anything lands
//! lock         held while refs change
//! ```

use crate::check::{run_checks, CheckSpec};
use crate::diff::{diff, Work};
use crate::graph::{ApplyError, LandedRename, View};
use crate::hash::Hash;
use crate::model::*;
use crate::source::is_zero_file;
use crate::store::{write_atomic, Store};
use crate::workdir::{materialize, scan, Ignore};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_WORKSPACE: &str = "default";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refs {
    pub main: Option<Hash>,
    #[serde(default)]
    pub drafts: BTreeMap<ChangeId, Hash>,
    #[serde(default)]
    pub workspaces: BTreeMap<String, Workspace>,
    #[serde(default)]
    pub conflicts: BTreeMap<String, Hash>,
    /// Where each named remote's main was when this repository last talked
    /// to it (like git's `origin/main`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remotes: BTreeMap<String, Hash>,
}

/// A working copy: a folder, the landing it was last synced to, the actor
/// who works in it, and its change in progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub path: String,
    pub base: Hash,
    #[serde(default)]
    pub actor: Option<Actor>,
    #[serde(default)]
    pub draft: Option<ChangeId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub checks: Vec<CheckSpec>,
    /// Named remotes: a GroveHub URL or another repository's folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remotes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpEntry {
    pub seq: u64,
    pub kind: String,
    pub summary: String,
    pub actor: String,
    pub at: u64,
    pub before: Refs,
    pub after: Refs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<u64>,
}

/// What happened when a change tried to land.
#[derive(Clone, Debug)]
pub enum LandOutcome {
    Landed {
        change: Change,
        mode: LandMode,
        evidence: Vec<Evidence>,
        landing: Hash,
        synced: Vec<String>,
    },
    Rejected {
        change: Change,
        evidence: Vec<Evidence>,
    },
    Conflict {
        change: Change,
        id: String,
        conflict: Conflict,
    },
}

#[derive(Clone, Debug)]
pub struct Repo {
    pub root: PathBuf,
    pub dir: PathBuf,
    pub store: Store,
}

#[derive(Serialize, Deserialize)]
struct WorkspaceFile {
    repo: String,
    name: String,
}

impl Repo {
    // ---------------------------------------------------------------- setup

    /// Start a repository in `root`, recording what is already there as the
    /// first landing.
    pub fn init(root: &Path, actor: &Actor, checks: Vec<CheckSpec>) -> Result<Repo> {
        let root = std::path::absolute(root)?;
        let dir = root.join(".grove");
        if dir.exists() {
            bail!("{} is already a Grove repository", root.display());
        }
        fs::create_dir_all(dir.join("objects"))?;
        let repo = Repo {
            store: Store::new(dir.join("objects")),
            root: root.clone(),
            dir,
        };
        write_atomic(
            &repo.dir.join("config.json"),
            &serde_json::to_vec_pretty(&Config {
                checks,
                ..Config::default()
            })?,
        )?;

        let work = scan(&root, &Ignore::load(&root, vec![]))?;
        let mut view = View::default();
        let ops = diff(&repo.store, &view, &work);
        view.apply(&repo.store, &ops, &[]).map_err(|e| anyhow!(e))?;
        let snapshot = repo.store.put_json("snapshot", &view.save(&repo.store)?)?;
        let landing = Landing {
            snapshot,
            parent: None,
            change: None,
            mode: LandMode::Init,
            evidence: vec![],
            at: now(),
        };
        let lh = repo.store.put_json("landing", &landing)?;

        let mut refs = Refs {
            main: Some(lh.clone()),
            ..Refs::default()
        };
        refs.workspaces.insert(
            DEFAULT_WORKSPACE.into(),
            Workspace {
                path: root.to_string_lossy().into(),
                base: lh,
                actor: Some(actor.clone()),
                draft: None,
            },
        );
        repo.write_refs(&refs)?;
        repo.append_op(OpEntry {
            seq: 1,
            kind: "init".into(),
            summary: format!("started with {} declarations", view.decls.len()),
            actor: actor.to_string(),
            at: now(),
            before: Refs::default(),
            after: refs,
            undoes: None,
        })?;
        Ok(repo)
    }

    /// Find the repository (and workspace) that contains `start`.
    pub fn discover(start: &Path) -> Result<(Repo, String)> {
        let start = std::path::absolute(start)?;
        let mut dir = Some(start.as_path());
        while let Some(d) = dir {
            if d.join(".grove").is_dir() {
                let repo = Repo::open(d)?;
                let refs = repo.refs()?;
                let ws = repo
                    .workspace_at(&refs, &start)
                    .unwrap_or_else(|| DEFAULT_WORKSPACE.into());
                return Ok((repo, ws));
            }
            let marker = d.join(".grove-workspace");
            if marker.is_file() {
                let wf: WorkspaceFile = serde_json::from_slice(&fs::read(&marker)?)?;
                return Ok((Repo::open(Path::new(&wf.repo))?, wf.name));
            }
            dir = d.parent();
        }
        bail!("not inside a Grove repository (run `grove init` first)")
    }

    pub fn open(root: &Path) -> Result<Repo> {
        let root = std::path::absolute(root)?;
        let dir = root.join(".grove");
        if !dir.is_dir() {
            bail!("{} is not a Grove repository", root.display());
        }
        Ok(Repo {
            store: Store::new(dir.join("objects")),
            root,
            dir,
        })
    }

    fn workspace_at(&self, refs: &Refs, path: &Path) -> Option<String> {
        refs.workspaces
            .iter()
            .filter(|(_, w)| path.starts_with(&w.path))
            .max_by_key(|(_, w)| w.path.len())
            .map(|(n, _)| n.clone())
    }

    pub fn config(&self) -> Result<Config> {
        let p = self.dir.join("config.json");
        if !p.exists() {
            return Ok(Config::default());
        }
        Ok(serde_json::from_slice(&fs::read(p)?)?)
    }

    // ----------------------------------------------------------- refs & log

    pub fn refs(&self) -> Result<Refs> {
        Ok(serde_json::from_slice(
            &fs::read(self.dir.join("refs.json")).context("missing refs")?,
        )?)
    }

    pub(crate) fn write_refs(&self, refs: &Refs) -> Result<()> {
        write_atomic(
            &self.dir.join("refs.json"),
            &serde_json::to_vec_pretty(refs)?,
        )
    }

    pub fn oplog(&self) -> Result<Vec<OpEntry>> {
        let p = self.dir.join("oplog.jsonl");
        if !p.exists() {
            return Ok(vec![]);
        }
        fs::read_to_string(p)?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| Ok(serde_json::from_str(l)?))
            .collect()
    }

    pub(crate) fn append_op(&self, entry: OpEntry) -> Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("oplog.jsonl"))?;
        writeln!(f, "{}", serde_json::to_string(&entry)?)?;
        Ok(())
    }

    /// Change refs under the lock and log the operation, so it can be undone.
    pub(crate) fn transact<T>(
        &self,
        kind: &str,
        actor: &str,
        f: impl FnOnce(&mut Refs) -> Result<(T, String)>,
    ) -> Result<T> {
        let _lock = Lock::acquire(&self.dir)?;
        let before = self.refs()?;
        let mut after = before.clone();
        let (value, summary) = f(&mut after)?;
        if after != before {
            self.write_refs(&after)?;
            let seq = self.oplog()?.last().map(|e| e.seq + 1).unwrap_or(1);
            self.append_op(OpEntry {
                seq,
                kind: kind.into(),
                summary,
                actor: actor.into(),
                at: now(),
                before,
                after,
                undoes: None,
            })?;
        }
        Ok(value)
    }

    // -------------------------------------------------------------- reading

    pub fn landing(&self, h: &Hash) -> Result<Landing> {
        self.store.get_json("landing", h)
    }

    pub fn change(&self, h: &Hash) -> Result<Change> {
        self.store.get_json("change", h)
    }

    pub fn view_at(&self, landing: &Hash) -> Result<View> {
        let l = self.landing(landing)?;
        View::load(&self.store, &self.store.get_json("snapshot", &l.snapshot)?)
    }

    pub fn main(&self) -> Result<Hash> {
        self.refs()?.main.ok_or_else(|| anyhow!("main is empty"))
    }

    pub fn main_view(&self) -> Result<View> {
        self.view_at(&self.main()?)
    }

    pub(crate) fn workspace(&self, refs: &Refs, name: &str) -> Result<Workspace> {
        refs.workspaces
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("no workspace named {name}"))
    }

    /// The working copy of a workspace, as declarations.
    pub fn read_workspace(&self, refs: &Refs, name: &str) -> Result<Work> {
        let ws = self.workspace(refs, name)?;
        let path = PathBuf::from(&ws.path);
        // a workspace never reads another workspace nested inside it
        let nested = refs
            .workspaces
            .values()
            .filter(|w| w.path != ws.path)
            .map(|w| PathBuf::from(&w.path))
            .filter(|p| p.starts_with(&path))
            .collect();
        scan(&path, &Ignore::load(&path, nested))
    }

    /// Landings from main back to `stop` (exclusive), newest first.
    pub fn history(&self, from: &Hash, stop: Option<&Hash>) -> Result<Vec<(Hash, Landing)>> {
        let mut out = vec![];
        let mut cur = Some(from.clone());
        while let Some(h) = cur {
            if Some(&h) == stop {
                return Ok(out);
            }
            let l = self.landing(&h)?;
            cur = l.parent.clone();
            out.push((h, l));
        }
        match stop {
            Some(s) => bail!("landing {} is not on main's history", s.short()),
            None => Ok(out),
        }
    }

    /// Changes that landed after `base`, oldest first.
    pub(crate) fn landed_since(&self, main: &Hash, base: &Hash) -> Result<Vec<Change>> {
        let mut out = vec![];
        for (_, l) in self.history(main, Some(base))?.into_iter().rev() {
            if let Some(c) = &l.change {
                out.push(self.change(c)?);
            }
        }
        Ok(out)
    }

    fn renames(changes: &[Change]) -> Vec<LandedRename> {
        changes
            .iter()
            .flat_map(|c| &c.ops)
            .filter_map(|op| match op {
                Op::Rename { from, to, .. } => Some(LandedRename {
                    from: from.clone(),
                    to: to.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    // ------------------------------------------------------------- commands

    /// Land a whole tree as one change straight onto main, without a
    /// workspace. Used by imports, where history is already decided.
    /// Returns `None` when the tree matches main.
    pub fn commit_tree(
        &self,
        work: &Work,
        actor: &Actor,
        intent: &str,
        at: u64,
        evidence: Vec<Evidence>,
    ) -> Result<Option<Hash>> {
        self.transact("import", &actor.to_string(), |refs| {
            let main = refs.main.clone().ok_or_else(|| anyhow!("main is empty"))?;
            let base = self.view_at(&main)?;
            let ops = diff(&self.store, &base, work);
            if ops.is_empty() {
                return Ok((None, String::new()));
            }
            let mut after = base.clone();
            let touched = after
                .apply(&self.store, &ops, &[])
                .map_err(|e| anyhow!(e))?;
            let footprint = footprint(&base, &after, &ops, &touched);
            let change = Change {
                id: fresh_id("c_", intent),
                version: 1,
                base: main.clone(),
                actor: actor.clone(),
                intent: intent.into(),
                ops,
                footprint,
                created: at,
            };
            let ch = self.store.put_json("change", &change)?;
            let snapshot = self.store.put_json("snapshot", &after.save(&self.store)?)?;
            let lh = self.store.put_json(
                "landing",
                &Landing {
                    snapshot,
                    parent: Some(main),
                    change: Some(ch),
                    mode: LandMode::Direct,
                    evidence,
                    at,
                },
            )?;
            refs.main = Some(lh.clone());
            Ok((Some(lh), format!("imported \"{intent}\"")))
        })
    }

    /// Point a workspace at main and write main into its folder.
    pub fn reset_workspace(&self, ws: &str, actor: &Actor) -> Result<()> {
        self.transact("reset", &actor.to_string(), |refs| {
            let main = refs.main.clone().ok_or_else(|| anyhow!("main is empty"))?;
            let w = self.workspace(refs, ws)?;
            let old = self.view_at(&w.base)?;
            materialize(Path::new(&w.path), &self.view_at(&main)?, Some(&old))?;
            refs.workspaces.get_mut(ws).unwrap().base = main;
            Ok(((), format!("reset {ws} to main")))
        })
    }

    /// What a workspace has changed since its base, as ops.
    pub fn status(&self, ws: &str) -> Result<Vec<Op>> {
        let refs = self.refs()?;
        let w = self.workspace(&refs, ws)?;
        let base = self.view_at(&w.base)?;
        Ok(diff(&self.store, &base, &self.read_workspace(&refs, ws)?))
    }

    /// Record a workspace's edits as a draft change (a new version of its
    /// current draft, if it has one).
    pub fn record(&self, ws: &str, intent: &str, actor: &Actor) -> Result<Change> {
        self.transact("record", &actor.to_string(), |refs| {
            let w = self.workspace(refs, ws)?;
            let base = self.view_at(&w.base)?;
            let ops = diff(&self.store, &base, &self.read_workspace(refs, ws)?);
            if ops.is_empty() {
                bail!("nothing to record: the workspace matches its base");
            }
            let mut after = base.clone();
            let touched = after
                .apply(&self.store, &ops, &[])
                .map_err(|e| anyhow!(e))?;
            let footprint = footprint(&base, &after, &ops, &touched);

            let (id, version) = match w
                .draft
                .as_ref()
                .and_then(|d| refs.drafts.get(d).map(|h| (d, h)))
            {
                Some((d, h)) => (d.clone(), self.change(h)?.version + 1),
                None => (fresh_id("c_", intent), 1),
            };
            let change = Change {
                id: id.clone(),
                version,
                base: w.base.clone(),
                actor: actor.clone(),
                intent: intent.into(),
                ops,
                footprint,
                created: now(),
            };
            let h = self.store.put_json("change", &change)?;
            refs.drafts.insert(id.clone(), h);
            refs.workspaces.get_mut(ws).unwrap().draft = Some(id.clone());
            let summary = format!("recorded {} v{} \"{}\"", short(&id), version, intent);
            Ok((change, summary))
        })
    }

    /// Land a draft on main: replay it on whatever landed since its base,
    /// order it after anything it depends on, check the combined result, and
    /// move main only if every check passes.
    pub fn land(&self, ws: &str, change_id: Option<&str>, actor: &Actor) -> Result<LandOutcome> {
        let config = self.config()?;
        self.transact("land", &actor.to_string(), |refs| {
            let id = match change_id {
                Some(c) => resolve_change(refs, c)?,
                None => self.workspace(refs, ws)?.draft.ok_or_else(|| {
                    anyhow!("nothing to land: record a change with `grove new` first")
                })?,
            };
            let change = self.change(
                refs.drafts
                    .get(&id)
                    .ok_or_else(|| anyhow!("no draft {}", short(&id)))?,
            )?;
            let main = refs.main.clone().ok_or_else(|| anyhow!("main is empty"))?;
            let since = self.landed_since(&main, &change.base)?;
            let renames = Self::renames(&since);
            let base_view = self.view_at(&change.base)?;
            let main_view = self.view_at(&main)?;

            // Both sides set the same declaration: a real conflict, unless
            // they set it to the same thing.
            let mut mine = base_view.clone();
            mine.apply(&self.store, &change.ops, &[])
                .map_err(|e| anyhow!(e))?;
            let mut clash: BTreeSet<DeclId> = BTreeSet::new();
            let mut against = BTreeSet::new();
            for l in &since {
                for d in change.footprint.writes.intersection(&l.footprint.writes) {
                    let same = match (mine.decls.get(d), main_view.decls.get(d)) {
                        (Some(a), Some(b)) => a.source == b.source,
                        (None, None) => true,
                        _ => false,
                    };
                    if !same {
                        clash.insert(d.clone());
                        against.insert(l.id.clone());
                    }
                }
            }
            let identical: BTreeSet<DeclId> = since
                .iter()
                .flat_map(|l| {
                    change
                        .footprint
                        .writes
                        .intersection(&l.footprint.writes)
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .filter(|d| !clash.contains(d))
                .collect();

            let mut view = main_view.clone();
            let ops: Vec<Op> = change
                .ops
                .iter()
                .filter(|op| !identical.contains(op.target()))
                .cloned()
                .collect();
            let applied = if clash.is_empty() {
                view.apply(&self.store, &ops, &renames)
            } else {
                Err(ApplyError::Missing { id: String::new() })
            };
            let touched = match applied {
                Ok(t) => t,
                Err(err) => {
                    if clash.is_empty() {
                        // a stale op the footprints did not predict
                        let (ApplyError::Stale { id, .. }
                        | ApplyError::Missing { id }
                        | ApplyError::Exists { id }) = err;
                        clash.insert(id);
                        against.extend(since.iter().map(|c| c.id.clone()));
                    }
                    let conflict = Conflict {
                        change: change.id.clone(),
                        against: against.into_iter().collect(),
                        decls: clash
                            .iter()
                            .map(|d| ConflictDecl {
                                id: d.clone(),
                                name: [&mine, &main_view, &base_view]
                                    .iter()
                                    .find_map(|v| v.decls.get(d))
                                    .map(|n| n.entry.name.clone())
                                    .unwrap_or_default(),
                                base: base_view.decls.get(d).map(|n| n.source.clone()),
                                main: main_view.decls.get(d).map(|n| n.source.clone()),
                                mine: mine.decls.get(d).map(|n| n.source.clone()),
                            })
                            .collect(),
                        at: now(),
                    };
                    let cid = fresh_id("x_", &change.id);
                    refs.conflicts
                        .insert(cid.clone(), self.store.put_json("conflict", &conflict)?);
                    let names: Vec<_> = conflict.decls.iter().map(|d| d.name.clone()).collect();
                    let summary = format!("conflict {} on {}", short(&cid), names.join(", "));
                    return Ok((
                        LandOutcome::Conflict {
                            change,
                            id: cid,
                            conflict,
                        },
                        summary,
                    ));
                }
            };

            // Order: did anything that landed since touch what this depends on?
            let depends: BTreeSet<DeclId> = change
                .footprint
                .touches()
                .into_iter()
                .chain(touched.iter().cloned())
                .collect();
            let (mut ordered, mut together) = (vec![], vec![]);
            for l in &since {
                let theirs = l.footprint.modifies();
                let hits_mine = depends.iter().any(|d| theirs.contains(d));
                let hits_theirs = change
                    .footprint
                    .modifies()
                    .iter()
                    .any(|d| l.footprint.reads.contains(d));
                if hits_mine || hits_theirs {
                    ordered.push(l.id.clone());
                } else {
                    together.push(l.id.clone());
                }
            }
            let mode = if !ordered.is_empty() {
                LandMode::Ordered { after: ordered }
            } else if !together.is_empty() {
                LandMode::Together { with: together }
            } else {
                LandMode::Direct
            };

            let evidence = run_checks(&view, &config.checks)?;
            if evidence.iter().any(|e| !e.ok) {
                return Ok((LandOutcome::Rejected { change, evidence }, String::new()));
            }

            let snapshot = self.store.put_json("snapshot", &view.save(&self.store)?)?;
            let landing = Landing {
                snapshot,
                parent: Some(main.clone()),
                change: Some(refs.drafts[&id].clone()),
                mode: mode.clone(),
                evidence: evidence.clone(),
                at: now(),
            };
            let lh = self.store.put_json("landing", &landing)?;
            refs.main = Some(lh.clone());
            refs.drafts.remove(&id);

            // Workspaces whose only edits were this change move to the new
            // main, so the next change starts from what everyone sees.
            let mut synced = vec![];
            let names: Vec<String> = refs
                .workspaces
                .iter()
                .filter(|(_, w)| w.draft.as_deref() == Some(&id))
                .map(|(n, _)| n.clone())
                .collect();
            for name in names {
                refs.workspaces.get_mut(&name).unwrap().draft = None;
                let w = refs.workspaces[&name].clone();
                let work = self.read_workspace(refs, &name)?;
                if work_files(&work) == mine.files() {
                    materialize(Path::new(&w.path), &view, Some(&mine))?;
                    refs.workspaces.get_mut(&name).unwrap().base = lh.clone();
                    synced.push(name);
                }
            }
            let summary = format!(
                "landed {} \"{}\" ({})",
                short(&change.id),
                change.intent,
                mode_word(&mode)
            );
            Ok((
                LandOutcome::Landed {
                    change,
                    mode,
                    evidence,
                    landing: lh,
                    synced,
                },
                summary,
            ))
        })
    }

    /// Bring a workspace up to main, carrying its unrecorded edits across.
    pub fn sync(&self, ws: &str, actor: &Actor) -> Result<String> {
        self.transact("sync", &actor.to_string(), |refs| {
            let w = self.workspace(refs, ws)?;
            let main = refs.main.clone().ok_or_else(|| anyhow!("main is empty"))?;
            if w.base == main {
                return Ok(("already up to date".to_string(), String::new()));
            }
            let base = self.view_at(&w.base)?;
            let ops = diff(&self.store, &base, &self.read_workspace(refs, ws)?);
            let since = self.landed_since(&main, &w.base)?;
            let mut result = self.view_at(&main)?;
            result.apply(&self.store, &ops, &Self::renames(&since)).map_err(|e| anyhow!("your edits clash with main: {e}. Record and land them, or undo them, then sync"))?;
            let mut mine = base.clone();
            mine.apply(&self.store, &ops, &[]).map_err(|e| anyhow!(e))?;
            materialize(Path::new(&w.path), &result, Some(&mine))?;
            refs.workspaces.get_mut(ws).unwrap().base = main;
            let msg = format!("synced {ws} past {} change(s), keeping {} edit(s) of your own", since.len(), ops.len());
            Ok((msg.clone(), msg))
        })
    }

    /// Undo the most recent operation that hasn't been undone.
    pub fn undo(&self, actor: &Actor) -> Result<String> {
        let _lock = Lock::acquire(&self.dir)?;
        let log = self.oplog()?;
        let undone: BTreeSet<u64> = log.iter().filter_map(|e| e.undoes).collect();
        let target = log
            .iter()
            .rev()
            .find(|e| e.kind != "undo" && e.kind != "init" && !undone.contains(&e.seq))
            .ok_or_else(|| anyhow!("nothing to undo"))?
            .clone();
        let current = self.refs()?;
        if current != target.after {
            bail!(
                "refs changed after op {}; undo the later operations first",
                target.seq
            );
        }
        self.write_refs(&target.before)?;
        let seq = log.last().map(|e| e.seq + 1).unwrap_or(1);
        let summary = format!("undid op {} ({})", target.seq, target.summary);
        self.append_op(OpEntry {
            seq,
            kind: "undo".into(),
            summary: summary.clone(),
            actor: actor.to_string(),
            at: now(),
            before: target.after.clone(),
            after: target.before.clone(),
            undoes: Some(target.seq),
        })?;
        Ok(summary)
    }

    /// Add a workspace: a new folder holding main, for a person or an agent.
    pub fn add_workspace(&self, name: &str, path: &Path, actor: &Actor) -> Result<PathBuf> {
        let path = std::path::absolute(path)?;
        self.transact("workspace", &actor.to_string(), |refs| {
            if refs.workspaces.contains_key(name) {
                bail!("workspace {name} already exists");
            }
            if path.exists() && fs::read_dir(&path)?.next().is_some() {
                bail!("{} is not empty", path.display());
            }
            fs::create_dir_all(&path)?;
            let main = refs.main.clone().ok_or_else(|| anyhow!("main is empty"))?;
            materialize(&path, &self.view_at(&main)?, None)?;
            let marker = WorkspaceFile {
                repo: self.root.to_string_lossy().into(),
                name: name.into(),
            };
            fs::write(
                path.join(".grove-workspace"),
                serde_json::to_vec_pretty(&marker)?,
            )?;
            refs.workspaces.insert(
                name.into(),
                Workspace {
                    path: path.to_string_lossy().into(),
                    base: main,
                    actor: Some(actor.clone()),
                    draft: None,
                },
            );
            Ok((path.clone(), format!("added workspace {name} for {actor}")))
        })
    }

    /// Who last set this declaration, and why. With `all`, every change that
    /// touched it, including renames that only updated a reference.
    pub fn why(&self, name: &str, all: bool) -> Result<(DeclId, Vec<(Landing, Change)>)> {
        let main = self.main()?;
        let id = self
            .view_at(&main)?
            .find(name)
            .ok_or_else(|| anyhow!("nothing named {name} on main"))?;
        let mut out = vec![];
        for (_, l) in self.history(&main, None)? {
            if let Some(ch) = &l.change {
                let c = self.change(ch)?;
                if c.footprint.writes.contains(&id) || (all && c.footprint.rewrites.contains(&id)) {
                    out.push((l, c));
                    if !all {
                        break;
                    }
                }
            }
        }
        Ok((id, out))
    }

    /// Find a change by id prefix among drafts and main's history.
    pub fn find_change(&self, prefix: &str) -> Result<Change> {
        let refs = self.refs()?;
        if let Ok(id) = resolve_change(&refs, prefix) {
            return self.change(&refs.drafts[&id]);
        }
        for (_, l) in self.history(&self.main()?, None)? {
            if let Some(h) = &l.change {
                let c = self.change(h)?;
                if c.id.starts_with(prefix) || short(&c.id).starts_with(prefix) {
                    return Ok(c);
                }
            }
        }
        bail!("no change matching {prefix}")
    }

    pub fn conflict(&self, prefix: &str) -> Result<(String, Conflict)> {
        let refs = self.refs()?;
        let (id, h) = refs
            .conflicts
            .iter()
            .find(|(id, _)| id.starts_with(prefix) || short(id).starts_with(prefix))
            .ok_or_else(|| anyhow!("no open conflict matching {prefix}"))?;
        Ok((id.clone(), self.store.get_json("conflict", h)?))
    }

    /// Close a conflict once it has been resolved (by landing a new version
    /// of the change, or by dropping it).
    pub fn close_conflict(&self, prefix: &str, actor: &Actor) -> Result<String> {
        let (id, _) = self.conflict(prefix)?;
        self.transact("resolve", &actor.to_string(), |refs| {
            refs.conflicts.remove(&id);
            Ok((
                format!("closed conflict {}", short(&id)),
                format!("closed conflict {}", short(&id)),
            ))
        })
    }

    /// Check every object still matches its address.
    pub fn verify(&self) -> Result<(usize, Vec<String>)> {
        let mut bad = vec![];
        let objects = self.store.all()?;
        for p in &objects {
            let bytes = fs::read(p)?;
            let name = format!(
                "{}{}",
                p.parent()
                    .and_then(|d| d.file_name())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                p.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
            let ok = ["decl", "snapshot", "change", "landing", "conflict"]
                .iter()
                .any(|d| Hash::of(d, &bytes).hex() == name);
            if !ok {
                bad.push(name);
            }
        }
        Ok((objects.len(), bad))
    }
}

/// What a change touches, computed from the graph before and after it.
pub fn footprint(before: &View, after: &View, ops: &[Op], touched: &BTreeSet<DeclId>) -> Footprint {
    let writes: BTreeSet<DeclId> = ops.iter().map(|op| op.target().clone()).collect();
    let rewrites: BTreeSet<DeclId> = touched.difference(&writes).cloned().collect();
    let changed: BTreeSet<DeclId> = writes.union(&rewrites).cloned().collect();
    let reads = before
        .neighbours(&changed)
        .union(&after.neighbours(&changed))
        .filter(|d| !changed.contains(*d))
        .cloned()
        .collect();
    Footprint {
        writes,
        rewrites,
        reads,
    }
}

/// Files as a working copy would hold them, joined the way views join them.
pub fn work_files(work: &Work) -> BTreeMap<String, String> {
    work.iter()
        .map(|(file, items)| {
            let text = if is_zero_file(file) {
                items
                    .iter()
                    .map(|i| i.source.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                items.iter().map(|i| i.source.as_str()).collect::<String>()
            };
            (file.clone(), text)
        })
        .collect()
}

fn resolve_change(refs: &Refs, prefix: &str) -> Result<ChangeId> {
    let hits: Vec<&ChangeId> = refs
        .drafts
        .keys()
        .filter(|id| id.starts_with(prefix) || short(id).starts_with(prefix))
        .collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!("no draft matching {prefix}"),
        _ => bail!("{prefix} matches more than one draft"),
    }
}

/// A change or conflict id without its prefix, for people: `c_1a2b3c`.
pub fn short(id: &str) -> String {
    match id.split_once('_') {
        Some((p, rest)) => format!("{p}_{}", &rest[..rest.len().min(6)]),
        None => id.chars().take(8).collect(),
    }
}

pub fn mode_word(mode: &LandMode) -> String {
    match mode {
        LandMode::Init => "first landing".into(),
        LandMode::Direct => "direct".into(),
        LandMode::Together { with } => format!(
            "together with {}",
            with.iter().map(|i| short(i)).collect::<Vec<_>>().join(", ")
        ),
        LandMode::Ordered { after } => format!(
            "ordered after {}",
            after
                .iter()
                .map(|i| short(i))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// An exclusive lock on the repository's refs, released on drop.
pub(crate) struct Lock {
    path: PathBuf,
}

impl Lock {
    pub(crate) fn acquire(dir: &Path) -> Result<Lock> {
        let path = dir.join("lock");
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Ok(Lock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // a lock older than two minutes belongs to a crashed process
                    let stale = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(120))
                        .unwrap_or(false);
                    if stale {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() > Duration::from_secs(30) {
                        bail!(
                            "the repository is busy (another grove command holds {})",
                            path.display()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

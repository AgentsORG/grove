//! Talking to another Grove repository: a GroveHub server, or another folder.
//!
//! The protocol moves **objects** (declarations, snapshots, changes,
//! landings) and **proposals** (one change, to be landed by the other side).
//!
//! - `fetch` sends every object reachable from the remote's main that a
//!   repository at `have` lacks. The receiver re-hashes every object against
//!   its domain and address, so a remote can never hand over a forged object.
//! - `push` fast-forwards the remote's main to history landed locally.
//! - `propose` sends one change. The remote replays, orders, checks and lands
//!   it with [`Repo::land`], the same code a laptop runs; or keeps it open for
//!   review. Landing logic is never duplicated on a server.
//!
//! The transport is the [`Remote`] trait. [`Folder`] talks to a repository on
//! disk; the `grove` CLI adds HTTP for GroveHub.

use crate::graph::View;
use crate::hash::Hash;
use crate::model::*;
use crate::repo::{work_files, Config, OpEntry, Refs, Repo, Workspace, DEFAULT_WORKSPACE};
use crate::store::write_atomic;
use crate::workdir::materialize;
use crate::LandOutcome;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// The kinds of object the protocol carries.
pub const DOMAINS: &[&str] = &["decl", "snapshot", "change", "landing", "conflict"];

/// One stored object on the wire: its domain, its address and its bytes
/// (every Grove object is UTF-8: source text or JSON).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Object {
    pub domain: String,
    pub hash: Hash,
    pub data: String,
}

/// A change waiting on the remote: recorded, sent, not landed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenChange {
    pub id: ChangeId,
    pub version: u32,
    pub intent: String,
    pub actor: Actor,
    pub change: Hash,
}

/// What a remote says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRefs {
    pub main: Option<Hash>,
    #[serde(default)]
    pub open: Vec<OpenChange>,
}

/// The objects a fetch brought, and the remote's main when it sent them.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Fetched {
    pub main: Option<Hash>,
    pub objects: Vec<Object>,
}

/// What happened to a proposed change on the remote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Proposal {
    /// It landed; `mode` says together with or ordered after what.
    Landed {
        change: ChangeId,
        intent: String,
        landing: Hash,
        mode: LandMode,
        evidence: Vec<Evidence>,
    },
    /// It is waiting for review.
    Open { change: ChangeId, version: u32 },
    /// It sets a declaration someone else already set differently. The
    /// conflict is saved on the remote for anyone to pick up.
    Conflict {
        change: ChangeId,
        conflict: String,
        decls: Vec<String>,
        against: Vec<ChangeId>,
    },
    /// A check failed on the combined result; main did not move.
    Rejected {
        change: ChangeId,
        evidence: Vec<Evidence>,
    },
}

impl Proposal {
    pub fn from_outcome(o: LandOutcome) -> Proposal {
        match o {
            LandOutcome::Landed {
                change,
                mode,
                evidence,
                landing,
                ..
            } => Proposal::Landed {
                change: change.id,
                intent: change.intent,
                landing,
                mode,
                evidence,
            },
            LandOutcome::Rejected { change, evidence } => Proposal::Rejected {
                change: change.id,
                evidence,
            },
            LandOutcome::Conflict {
                change,
                id,
                conflict,
            } => Proposal::Conflict {
                change: change.id,
                conflict: id,
                decls: conflict.decls.iter().map(|d| d.name.clone()).collect(),
                against: conflict.against,
            },
        }
    }
}

/// Another Grove repository this one can talk to.
pub trait Remote {
    fn refs(&self) -> Result<RemoteRefs>;
    /// Everything reachable from the remote's main that a repository whose
    /// main is `have` lacks.
    fn fetch(&self, have: Option<&Hash>) -> Result<Fetched>;
    /// Store `objects` on the remote and fast-forward its main to `main`.
    fn push(&self, main: &Hash, objects: Vec<Object>, actor: &Actor) -> Result<usize>;
    /// Send one change; land it now, or keep it open for review.
    fn propose(&self, change: &Change, land: bool, actor: &Actor) -> Result<Proposal>;
}

/// A remote that is another repository on this machine.
pub struct Folder(pub Repo);

impl Folder {
    pub fn open(path: &Path) -> Result<Folder> {
        Ok(Folder(Repo::open(path)?))
    }
}

impl Remote for Folder {
    fn refs(&self) -> Result<RemoteRefs> {
        self.0.remote_refs()
    }
    fn fetch(&self, have: Option<&Hash>) -> Result<Fetched> {
        let main = self.0.refs()?.main;
        let objects = match &main {
            Some(m) => self.0.objects_since(m, have)?,
            None => vec![],
        };
        Ok(Fetched { main, objects })
    }
    fn push(&self, main: &Hash, objects: Vec<Object>, actor: &Actor) -> Result<usize> {
        self.0.receive(&objects)?;
        self.0.fast_forward(main, actor)
    }
    fn propose(&self, change: &Change, land: bool, actor: &Actor) -> Result<Proposal> {
        self.0.propose(change, land, actor)
    }
}

/// What `push` did.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PushReport {
    /// Landings of local history sent to the remote's main.
    pub landings: usize,
    /// The change sent, and what became of it.
    pub proposal: Option<Proposal>,
    /// Workspaces moved to the new main after the change landed.
    pub synced: Vec<String>,
}

impl Repo {
    // ------------------------------------------------------------ the host

    /// A repository with no working copy and an empty main, for a server to
    /// host. Its first landing arrives by push.
    pub fn init_bare(root: &Path, actor: &Actor) -> Result<Repo> {
        let root = std::path::absolute(root)?;
        let dir = root.join(".grove");
        if dir.exists() {
            bail!("{} is already a Grove repository", root.display());
        }
        fs::create_dir_all(dir.join("objects"))?;
        let repo = Repo::open(&root)?;
        write_atomic(
            &repo.dir.join("config.json"),
            &serde_json::to_vec_pretty(&Config::default())?,
        )?;
        repo.write_refs(&Refs::default())?;
        repo.append_op(OpEntry {
            seq: 1,
            kind: "init".into(),
            summary: "started an empty repository".into(),
            actor: actor.to_string(),
            at: now(),
            before: Refs::default(),
            after: Refs::default(),
            undoes: None,
        })?;
        Ok(repo)
    }

    /// Main and the changes waiting on it.
    pub fn remote_refs(&self) -> Result<RemoteRefs> {
        let refs = self.refs()?;
        let mut open = vec![];
        for h in refs.drafts.values() {
            let c = self.change(h)?;
            open.push(OpenChange {
                id: c.id,
                version: c.version,
                intent: c.intent,
                actor: c.actor,
                change: h.clone(),
            });
        }
        Ok(RemoteRefs {
            main: refs.main,
            open,
        })
    }

    /// Every object reachable from `main` that a repository whose main is
    /// `have` doesn't already hold. If `have` isn't on main's history, all of
    /// it is sent.
    pub fn objects_since(&self, main: &Hash, have: Option<&Hash>) -> Result<Vec<Object>> {
        let mut landings = vec![];
        let mut cur = Some(main.clone());
        let mut found = false;
        while let Some(h) = cur {
            if Some(&h) == have {
                found = true;
                break;
            }
            let l = self.landing(&h)?;
            cur = l.parent.clone();
            landings.push((h, l));
        }
        // Declarations the other side already holds through its main.
        let mut sent: BTreeSet<Hash> = BTreeSet::new();
        if let (true, Some(have)) = (found, have) {
            let l = self.landing(have)?;
            let snap: Snapshot = self.store.get_json("snapshot", &l.snapshot)?;
            sent.extend(snap.decls.values().map(|e| e.content.clone()));
        }
        let mut decls = vec![];
        let mut rest = vec![];
        let raw = |domain: &str, h: &Hash| -> Result<Object> {
            Ok(Object {
                domain: domain.into(),
                hash: h.clone(),
                data: String::from_utf8(self.store.get_bytes(domain, h)?)
                    .with_context(|| format!("object {h} is not text"))?,
            })
        };
        // Oldest first, so a receiver can check each landing's parent.
        for (h, l) in landings.iter().rev() {
            let snap: Snapshot = self.store.get_json("snapshot", &l.snapshot)?;
            for e in snap.decls.values() {
                if sent.insert(e.content.clone()) {
                    decls.push(raw("decl", &e.content)?);
                }
            }
            rest.push(raw("snapshot", &l.snapshot)?);
            if let Some(c) = &l.change {
                rest.push(raw("change", c)?);
            }
            rest.push(raw("landing", h)?);
        }
        decls.extend(rest);
        Ok(decls)
    }

    /// Store objects from elsewhere, checking every one hashes to its address.
    /// Returns how many were new.
    pub fn receive(&self, objects: &[Object]) -> Result<usize> {
        let mut new = 0;
        for o in objects {
            if !DOMAINS.contains(&o.domain.as_str()) {
                bail!("unknown object kind {}", o.domain);
            }
            if Hash::of(&o.domain, o.data.as_bytes()) != o.hash {
                bail!("object {} does not match its address; refusing it", o.hash);
            }
            if !self.store.has(&o.hash) {
                self.store.put_bytes(&o.domain, o.data.as_bytes())?;
                new += 1;
            }
        }
        Ok(new)
    }

    /// Whether landing `a` is on the history of landing `b` (or is `b`).
    pub fn is_ancestor(&self, a: &Hash, b: &Hash) -> bool {
        self.history(b, Some(a)).is_ok()
    }

    /// Move main forward to `new_main`, which must have main on its history
    /// and be complete in the store. Returns how many landings it added.
    pub fn fast_forward(&self, new_main: &Hash, actor: &Actor) -> Result<usize> {
        self.transact("push", &actor.to_string(), |refs| {
            if refs.main.as_ref() == Some(new_main) {
                return Ok((0, String::new()));
            }
            let added = self
                .history(new_main, refs.main.as_ref())
                .map_err(|_| anyhow!("main has moved on; fetch and land your work on top of it"))?;
            // every landing must be whole: its snapshot and declarations
            for (h, _) in &added {
                self.view_at(h)
                    .with_context(|| format!("landing {} arrived incomplete", h.short()))?;
            }
            refs.main = Some(new_main.clone());
            let n = added.len();
            Ok((n, format!("main moved forward {n} landing(s)")))
        })
    }

    /// Take a change from someone else. It becomes an open change (a draft
    /// here); with `land`, it is landed at once.
    pub fn propose(&self, change: &Change, land: bool, actor: &Actor) -> Result<Proposal> {
        if !change.id.starts_with("c_") || change.id.len() > 64 {
            bail!("{} is not a change id", change.id);
        }
        let main = self
            .refs()?
            .main
            .ok_or_else(|| anyhow!("this repository is empty: push its history first"))?;
        if !self.store.has(&change.base) || !self.is_ancestor(&change.base, &main) {
            bail!(
                "change {} starts from landing {}, which isn't on main here; pull first",
                short(&change.id),
                change.base.short()
            );
        }
        // the change's ops must apply to its own base
        let mut check = self.view_at(&change.base)?;
        check.apply(&self.store, &change.ops, &[]).map_err(|e| {
            anyhow!(
                "change {} does not apply to its base: {e}",
                short(&change.id)
            )
        })?;

        let version = change.version;
        self.transact("propose", &actor.to_string(), |refs| {
            let h = self.store.put_json("change", change)?;
            refs.drafts.insert(change.id.clone(), h);
            Ok((
                (),
                format!(
                    "received {} v{} \"{}\" from {}",
                    short(&change.id),
                    version,
                    change.intent,
                    change.actor
                ),
            ))
        })?;
        if !land {
            return Ok(Proposal::Open {
                change: change.id.clone(),
                version,
            });
        }
        self.land_open(&change.id, actor)
    }

    /// Land a change that is waiting here.
    pub fn land_open(&self, id: &str, actor: &Actor) -> Result<Proposal> {
        Ok(Proposal::from_outcome(self.land(
            DEFAULT_WORKSPACE,
            Some(id),
            actor,
        )?))
    }

    // ---------------------------------------------------------- the client

    /// Add or replace a named remote in `.grove/config.json`.
    pub fn set_remote(&self, name: &str, url: &str) -> Result<()> {
        let mut config = self.config()?;
        config.remotes.insert(name.into(), url.into());
        write_atomic(
            &self.dir.join("config.json"),
            &serde_json::to_vec_pretty(&config)?,
        )
    }

    /// The URL or folder of a named remote.
    pub fn remote_url(&self, name: &str) -> Result<String> {
        self.config()?
            .remotes
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("no remote named {name} (add one with `grove remote add`)"))
    }

    /// Fetch everything new from a remote and remember where its main is.
    /// Main itself doesn't move. Returns the remote's main and how many
    /// objects were new.
    pub fn fetch_from(&self, name: &str, remote: &dyn Remote) -> Result<(Option<Hash>, usize)> {
        let have = self.refs()?.main;
        let got = remote.fetch(have.as_ref())?;
        let new = self.receive(&got.objects)?;
        if let Some(m) = &got.main {
            self.view_at(m)
                .with_context(|| format!("{name} sent an incomplete history"))?;
            let m = m.clone();
            let name = name.to_string();
            self.transact("fetch", "grove", move |refs| {
                refs.remotes.insert(name.clone(), m.clone());
                Ok(((), format!("fetched {name}: main at {}", m.short())))
            })?;
        }
        Ok((got.main, new))
    }

    /// Move main forward to a fetched landing. If a change of ours landed
    /// there, drop its draft and move the workspaces that held only it.
    pub fn adopt(
        &self,
        new_main: &Hash,
        landed: Option<&str>,
        actor: &Actor,
    ) -> Result<Vec<String>> {
        self.transact("pull", &actor.to_string(), |refs| {
            if let Some(m) = &refs.main {
                if !self.is_ancestor(m, new_main) {
                    bail!("main here has landings the remote doesn't; push them first");
                }
            }
            refs.main = Some(new_main.clone());
            let view = self.view_at(new_main)?;
            let mut synced = vec![];
            if let Some(id) = landed {
                let Some(h) = refs.drafts.remove(id) else {
                    return Ok((synced, format!("main at {}", new_main.short())));
                };
                let change = self.change(&h)?;
                let names: Vec<String> = refs
                    .workspaces
                    .iter()
                    .filter(|(_, w)| w.draft.as_deref() == Some(id))
                    .map(|(n, _)| n.clone())
                    .collect();
                for name in names {
                    refs.workspaces.get_mut(&name).unwrap().draft = None;
                    let w = refs.workspaces[&name].clone();
                    let mut mine = self.view_at(&change.base)?;
                    mine.apply(&self.store, &change.ops, &[])
                        .map_err(|e| anyhow!(e))?;
                    let work = self.read_workspace(refs, &name)?;
                    if work_files(&work) == mine.files() {
                        materialize(Path::new(&w.path), &view, Some(&mine))?;
                        refs.workspaces.get_mut(&name).unwrap().base = new_main.clone();
                        synced.push(name);
                    }
                }
            }
            let summary = format!("main moved to {} from the remote", new_main.short());
            Ok((synced, summary))
        })
    }

    /// Make a new repository at `dest` holding a remote's main, with a
    /// default workspace in `dest` for `actor`.
    pub fn clone_from(url: &str, remote: &dyn Remote, dest: &Path, actor: &Actor) -> Result<Repo> {
        if dest.exists() && fs::read_dir(dest)?.next().is_some() {
            bail!("{} is not empty", dest.display());
        }
        fs::create_dir_all(dest)?;
        let repo = Repo::init_bare(dest, actor)?;
        repo.set_remote("origin", url)?;
        let (main, _) = repo.fetch_from("origin", remote)?;
        let main = main.ok_or_else(|| {
            anyhow!("the remote is empty: push to it from an existing Grove repository")
        })?;
        let root = repo.root.clone();
        repo.transact("clone", &actor.to_string(), |refs| {
            refs.main = Some(main.clone());
            refs.workspaces.insert(
                DEFAULT_WORKSPACE.into(),
                Workspace {
                    path: root.to_string_lossy().into(),
                    base: main.clone(),
                    actor: Some(actor.clone()),
                    draft: None,
                },
            );
            materialize(&root, &repo.view_at(&main)?, None)?;
            Ok(((), format!("cloned {url}")))
        })?;
        Ok(repo)
    }

    /// Fetch, move main forward to the remote's, and sync the workspace.
    pub fn pull(&self, ws: &str, name: &str, remote: &dyn Remote, actor: &Actor) -> Result<String> {
        let (theirs, _) = self.fetch_from(name, remote)?;
        let Some(theirs) = theirs else {
            return Ok(format!("{name} is empty"));
        };
        let ours = self.refs()?.main;
        match &ours {
            Some(m) if *m == theirs => {}
            Some(m) if self.is_ancestor(&theirs, m) => {
                return Ok(format!(
                    "{name} is behind; `grove push` sends your landings"
                ))
            }
            Some(m) if !self.is_ancestor(m, &theirs) => {
                bail!("main here and on {name} have both moved; land your work through {name} instead (diverged pushes are on the roadmap)")
            }
            _ => {
                self.adopt(&theirs, None, actor)?;
            }
        }
        self.sync(ws, actor)
    }

    /// Send work to a remote: any landings it lacks (a fast-forward), then
    /// this workspace's change (or `change`), to land now or, with `review`,
    /// to wait there. When it lands, main here moves to the remote's.
    pub fn push_to(
        &self,
        ws: &str,
        name: &str,
        remote: &dyn Remote,
        change: Option<&str>,
        review: bool,
        actor: &Actor,
    ) -> Result<PushReport> {
        let mut report = PushReport::default();
        let theirs = remote.refs()?.main;
        let ours = self.main()?;
        if theirs.as_ref() != Some(&ours) {
            let ahead = match &theirs {
                None => true,
                Some(t) => self.store.has(t) && self.is_ancestor(t, &ours),
            };
            let behind = theirs
                .as_ref()
                .is_some_and(|t| self.store.has(t) && self.is_ancestor(&ours, t));
            if ahead {
                let objects = self.objects_since(&ours, theirs.as_ref())?;
                report.landings = remote.push(&ours, objects, actor)?;
                let ours2 = ours.clone();
                let name2 = name.to_string();
                self.transact("fetch", &actor.to_string(), move |refs| {
                    refs.remotes.insert(name2, ours2);
                    Ok(((), String::new()))
                })?;
            } else if !behind && theirs.as_ref().is_some_and(|t| self.store.has(t)) {
                bail!("main here and on {name} have both moved; land your work through {name} instead (diverged pushes are on the roadmap)");
            }
            // behind, or unknown to us: the change itself can still go, as
            // long as its base is on the remote's main.
        }

        let refs = self.refs()?;
        let id = match change {
            Some(c) => Some(
                refs.drafts
                    .keys()
                    .find(|id| id.starts_with(c) || short(id).starts_with(c))
                    .cloned()
                    .ok_or_else(|| anyhow!("no draft matching {c}"))?,
            ),
            None => refs.workspaces.get(ws).and_then(|w| w.draft.clone()),
        };
        let Some(id) = id else {
            return Ok(report);
        };
        let c = self.change(&refs.drafts[&id])?;
        let proposal = remote.propose(&c, !review, actor)?;
        if let Proposal::Landed { .. } = &proposal {
            let (theirs, _) = self.fetch_from(name, remote)?;
            if let Some(t) = theirs {
                report.synced = self.adopt(&t, Some(&id), actor)?;
            }
        }
        report.proposal = Some(proposal);
        Ok(report)
    }
}

use crate::repo::short;

/// A view of the program graph on main, for a remote that wants to show it.
pub fn main_view(repo: &Repo) -> Result<Option<View>> {
    match repo.refs()?.main {
        Some(m) => Ok(Some(repo.view_at(&m)?)),
        None => Ok(None),
    }
}

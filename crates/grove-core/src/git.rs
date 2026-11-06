//! The git bridge.
//!
//! GitHub, GitLab, Cursor, CI systems and every editor speak git, so Grove
//! has to as well. This module:
//!
//! - **exports** main's history as ordinary git commits (one per landing,
//!   with who, for whom, why and the evidence in trailers), plus tags;
//! - **pushes** that history to any git remote;
//! - **imports** a git repository's history as Grove changes, reusing the
//!   original commit ids when the trees match, so an import followed by an
//!   export gives back the same history;
//! - **pulls** new commits from a git remote. When Grove's main also moved,
//!   the commits are replayed like any change and exported as merge commits,
//!   so the next push fast-forwards instead of being refused.
//!
//! It drives the `git` command rather than reimplementing git's formats, so
//! what it produces is exactly what git itself would.

use crate::diff::{diff, Work};
use crate::graph::View;
use crate::hash::Hash;
use crate::model::{fresh_id, now, Actor, ActorKind, Change, Evidence};
use crate::repo::{footprint, mode_word, LandOutcome, Repo, DEFAULT_WORKSPACE};
use crate::source::split_bytes;
use crate::workdir::materialize;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(args: &[&str], dir: Option<&Path>, env: &[(&str, String)]) -> Result<String> {
    let mut c = Command::new("git");
    if let Some(d) = dir {
        c.current_dir(d);
    }
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c
        .args(args)
        .output()
        .context("could not run git (is it installed?)")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn email_for(actor: &Actor) -> String {
    let slug: String = actor
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    match actor.kind {
        ActorKind::Agent => format!("{slug}@agents.grove"),
        ActorKind::Person => format!("{slug}@people.grove"),
    }
}

/// What the bridge remembers between runs.
#[derive(Default, Serialize, Deserialize)]
struct State {
    /// Landing -> the git commit that represents it.
    #[serde(default)]
    map: BTreeMap<Hash, String>,
    /// Landing -> a git commit it merged in (from `pull`), exported as a
    /// second parent.
    #[serde(default)]
    merges: BTreeMap<Hash, String>,
}

/// The bare git repository Grove keeps in step with main.
pub struct GitMirror {
    pub dir: PathBuf,
    state_path: PathBuf,
    pub map: BTreeMap<Hash, String>,
    merges: BTreeMap<Hash, String>,
}

impl GitMirror {
    pub fn open(repo: &Repo) -> Result<GitMirror> {
        let dir = repo.dir.join("git");
        let state_path = repo.dir.join("git-map.json");
        if !dir.exists() {
            git(
                &[
                    "init",
                    "--bare",
                    "--quiet",
                    "--initial-branch=main",
                    &dir.to_string_lossy(),
                ],
                None,
                &[],
            )?;
        }
        let state: State = match std::fs::read(&state_path) {
            // older versions stored the map alone
            Ok(b) => serde_json::from_slice::<State>(&b).or_else(|_| {
                serde_json::from_slice::<BTreeMap<Hash, String>>(&b).map(|map| State {
                    map,
                    ..State::default()
                })
            })?,
            Err(_) => State::default(),
        };
        Ok(GitMirror {
            dir,
            state_path,
            map: state.map,
            merges: state.merges,
        })
    }

    fn save(&self) -> Result<()> {
        let state = State {
            map: self.map.clone(),
            merges: self.merges.clone(),
        };
        crate::store::write_atomic(&self.state_path, &serde_json::to_vec_pretty(&state)?)
    }

    fn gd(&self) -> String {
        format!("--git-dir={}", self.dir.display())
    }

    /// The git tree a view would produce.
    fn tree_of(&self, view: &View, key: &str) -> Result<String> {
        let gd = self.gd();
        let tree_dir = std::env::temp_dir().join(fresh_id("grove-tree-", key));
        std::fs::create_dir_all(&tree_dir)?;
        materialize(&tree_dir, view, None)?;
        let wt = format!("--work-tree={}", tree_dir.display());
        let index = self.dir.join(format!("index-{}", fresh_id("", key)));
        let env_index = [("GIT_INDEX_FILE", index.to_string_lossy().to_string())];
        let result = git(
            &[&gd, &wt, "add", "--all", "."],
            Some(&tree_dir),
            &env_index,
        )
        .and_then(|_| git(&[&gd, "write-tree"], Some(&tree_dir), &env_index));
        let _ = std::fs::remove_file(&index);
        let _ = std::fs::remove_dir_all(&tree_dir);
        Ok(result?.trim().to_string())
    }

    fn commit_tree_of(&self, sha: &str) -> Result<String> {
        Ok(git(
            &[&self.gd(), "rev-parse", &format!("{sha}^{{tree}}")],
            None,
            &[],
        )?
        .trim()
        .to_string())
    }
}

/// Write every landing not yet exported as a git commit on `main`, point
/// `main` at the commit for Grove's current main (so an undo shows up in git
/// as main moving back), and write Grove's tags as git tags. Returns the
/// mirror and how many commits were written.
pub fn export(repo: &Repo) -> Result<(GitMirror, usize)> {
    let mut mirror = GitMirror::open(repo)?;
    let main = repo.main()?;
    let history = repo.history(&main, None)?;
    let gd = mirror.gd();
    let mut written = 0;
    // An empty first landing (a repository started in an empty folder, or by
    // an import) has nothing to commit; its children become root commits.
    let empty_root = |h: &Hash| -> bool {
        repo.landing(h).map(|l| l.parent.is_none()).unwrap_or(false)
            && repo.view_at(h).map(|v| v.decls.is_empty()).unwrap_or(false)
    };
    for (h, landing) in history.iter().rev() {
        if mirror.map.contains_key(h) || empty_root(h) {
            continue;
        }
        let parent = match landing.parent.as_ref() {
            Some(p) if empty_root(p) => None,
            Some(p) => Some(
                mirror
                    .map
                    .get(p)
                    .cloned()
                    .ok_or_else(|| anyhow!("parent {} was not exported", p.short()))?,
            ),
            None => None,
        };
        let tree = mirror.tree_of(&repo.view_at(h)?, h.as_str())?;

        let (actor, message) = match &landing.change {
            Some(ch) => {
                let c = repo.change(ch)?;
                let mut m = format!("{}\n\n", c.intent);
                m.push_str(&format!("Grove-Change: {} v{}\n", c.id, c.version));
                m.push_str(&format!("Grove-Landing: {h}\n"));
                m.push_str(&format!("Grove-Mode: {}\n", mode_word(&landing.mode)));
                if c.actor.kind == ActorKind::Agent {
                    m.push_str(&format!("Grove-Agent: {}\n", c.actor.name));
                }
                if let Some(p) = &c.actor.for_person {
                    m.push_str(&format!("Grove-For: {p}\n"));
                }
                for e in &landing.evidence {
                    m.push_str(&format!(
                        "Grove-Evidence: {} {}\n",
                        if e.ok { "ok" } else { "failed" },
                        e.check
                    ));
                }
                (c.actor, m)
            }
            None => (
                Actor::person("grove"),
                format!("Start the Grove repository\n\nGrove-Landing: {h}\n"),
            ),
        };
        let date = format!("{} +0000", landing.at);
        let env = [
            ("GIT_AUTHOR_NAME", actor.name.clone()),
            ("GIT_AUTHOR_EMAIL", email_for(&actor)),
            ("GIT_AUTHOR_DATE", date.clone()),
            ("GIT_COMMITTER_NAME", "Grove".to_string()),
            ("GIT_COMMITTER_EMAIL", "grove@agents.grove".to_string()),
            ("GIT_COMMITTER_DATE", date),
        ];
        let mut args = vec![gd.as_str(), "commit-tree", tree.as_str()];
        if let Some(p) = &parent {
            args.push("-p");
            args.push(p);
        }
        let merged = mirror.merges.get(h).cloned();
        if let Some(m) = &merged {
            args.push("-p");
            args.push(m);
        }
        let mut child = Command::new("git")
            .args(&args)
            .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child.stdin.take().unwrap().write_all(message.as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!(
                "git commit-tree failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        mirror.map.insert(
            h.clone(),
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        );
        written += 1;
    }
    let Some(head) = mirror.map.get(&main).cloned() else {
        // main is still the empty first landing: nothing to export yet
        mirror.save()?;
        return Ok((mirror, written));
    };
    git(&[&gd, "update-ref", "refs/heads/main", &head], None, &[])?;
    // tags on landings that are on main's history
    for (name, h) in &repo.refs()?.tags {
        if let Some(sha) = mirror.map.get(h) {
            git(
                &[&gd, "update-ref", &format!("refs/tags/{name}"), sha],
                None,
                &[],
            )?;
        }
    }
    mirror.save()?;
    Ok((mirror, written))
}

/// Push main and tags to a git remote (GitHub, GitLab, ...).
pub fn push(repo: &Repo, remote: &str, force: bool) -> Result<String> {
    let (mirror, _) = export(repo)?;
    let gd = mirror.gd();
    let mut args = vec![
        gd.as_str(),
        "push",
        remote,
        "main:main",
        "refs/tags/*:refs/tags/*",
    ];
    if force {
        args.push("--force-with-lease");
    }
    git(&args, None, &[])?;
    Ok(format!("pushed main and tags to {remote}"))
}

/// What `pull` did.
#[derive(Debug, Default)]
pub struct Pulled {
    /// Commits that became landings directly (Grove's main hadn't moved).
    pub fast_forward: usize,
    /// Commits replayed on top of Grove's newer main (merged in git).
    pub replayed: usize,
    /// A commit that clashed with Grove's main; nothing after it was taken.
    pub conflict: Option<(String, String)>,
}

/// Bring in commits made on a git remote's `main` since the last export or
/// pull. Fast-forwards when Grove's main hasn't moved; otherwise replays each
/// commit as a change and records the git merge for the next export.
pub fn pull(repo: &Repo, remote: &str, actor: &Actor) -> Result<Pulled> {
    export(repo)?; // make sure every landing has a commit to compare with
    let mut mirror = GitMirror::open(repo)?;
    let gd = mirror.gd();
    git(
        &[
            &gd,
            "fetch",
            "--quiet",
            remote,
            "+refs/heads/main:refs/remotes/upstream/main",
        ],
        None,
        &[],
    )?;
    let upstream = git(&[&gd, "rev-parse", "refs/remotes/upstream/main"], None, &[])?
        .trim()
        .to_string();
    let by_commit: BTreeMap<String, Hash> = mirror
        .map
        .iter()
        .map(|(l, c)| (c.clone(), l.clone()))
        .chain(mirror.merges.iter().map(|(l, c)| (c.clone(), l.clone())))
        .collect();
    if by_commit.contains_key(&upstream) {
        return Ok(Pulled::default());
    }
    let mut args = vec![
        gd.clone(),
        "rev-list".into(),
        "--first-parent".into(),
        "--reverse".into(),
        "--format=%P%x1f%an%x1f%at%x1f%s".into(),
        upstream.clone(),
    ];
    args.extend(by_commit.keys().map(|c| format!("^{c}")));
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let listing = git(&argv, None, &[])?;
    // rev-list --format prints "commit <sha>" then the formatted line
    let mut commits = vec![];
    let mut lines = listing.lines();
    while let Some(head) = lines.next() {
        let Some(sha) = head.strip_prefix("commit ") else {
            continue;
        };
        let fmt = lines.next().unwrap_or("");
        let mut p = fmt.split('\u{1f}');
        let parents = p
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let author = p.next().unwrap_or("someone").to_string();
        let at: u64 = p.next().and_then(|s| s.parse().ok()).unwrap_or_else(now);
        let subject = p.next().unwrap_or("").to_string();
        commits.push((sha.to_string(), parents, author, at, subject));
    }

    let mut pulled = Pulled::default();
    let mut known = by_commit;
    for (sha, parents, author, at, subject) in commits {
        let parent = parents.first().ok_or_else(|| {
            anyhow!(
                "commit {sha} has no parent; pull into a fresh repository with `grove git import`"
            )
        })?;
        let base = known
            .get(parent)
            .cloned()
            .ok_or_else(|| anyhow!("commit {sha}'s parent isn't known to Grove"))?;
        let work = tree_at(&mirror.dir, &sha)?;
        let main = repo.main()?;
        let evidence = vec![Evidence {
            check: "pulled from git".into(),
            ok: true,
            detail: sha.clone(),
        }];
        if base == main {
            if let Some(lh) =
                repo.commit_tree(&work, &Actor::person(&author), &subject, at, evidence)?
            {
                // the landing *is* this commit when the trees match
                if mirror.tree_of(&repo.view_at(&lh)?, lh.as_str())?
                    == mirror.commit_tree_of(&sha)?
                {
                    mirror.map.insert(lh.clone(), sha.clone());
                } else {
                    mirror.merges.insert(lh.clone(), sha.clone());
                }
                known.insert(sha.clone(), lh);
            }
            pulled.fast_forward += 1;
            continue;
        }
        let base_view = repo.view_at(&base)?;
        let ops = diff(&repo.store, &base_view, &work);
        if ops.is_empty() {
            continue;
        }
        let mut after = base_view.clone();
        let touched = after
            .apply(&repo.store, &ops, &[])
            .map_err(|e| anyhow!(e))?;
        let change = Change {
            id: fresh_id("c_", &sha),
            version: 1,
            base,
            actor: Actor::person(&author),
            intent: subject.clone(),
            footprint: footprint(&base_view, &after, &ops, &touched),
            ops,
            created: at,
            previous: None,
            signature: None,
        };
        repo.stage(&change, actor)?;
        match repo.land_on(None, DEFAULT_WORKSPACE, Some(&change.id), actor)? {
            LandOutcome::Landed { landing, .. } => {
                mirror.merges.insert(landing.clone(), sha.clone());
                known.insert(sha.clone(), landing);
                pulled.replayed += 1;
            }
            LandOutcome::Conflict { id, .. } => {
                pulled.conflict = Some((sha, id));
                break;
            }
            LandOutcome::Rejected { .. } => {
                pulled.conflict = Some((sha, "a check failed".into()));
                break;
            }
        }
    }
    mirror.save()?;
    Ok(pulled)
}

/// Create a Grove repository at `dest` from a git repository's first-parent
/// history: one landed change per commit, keeping each commit's author, date
/// and subject. Commits whose trees Grove reproduces exactly keep their ids
/// when exported again.
pub fn import(
    source: &Path,
    dest: &Path,
    importer: &Actor,
    limit: Option<usize>,
) -> Result<(Repo, usize)> {
    let source = std::path::absolute(source)?;
    let log = git(
        &[
            "log",
            "--first-parent",
            "--reverse",
            "--format=%H%x1f%an%x1f%at%x1f%s",
            "HEAD",
        ],
        Some(&source),
        &[],
    )?;
    let mut commits: Vec<(String, String, u64, String)> = log
        .lines()
        .filter_map(|l| {
            let mut p = l.split('\u{1f}');
            Some((
                p.next()?.to_string(),
                p.next()?.to_string(),
                p.next()?.parse().unwrap_or(0),
                p.next().unwrap_or("").to_string(),
            ))
        })
        .collect();
    if let Some(n) = limit {
        let skip = commits.len().saturating_sub(n);
        commits.drain(..skip);
    }
    if commits.is_empty() {
        bail!("{} has no commits", source.display());
    }
    if dest.exists() && std::fs::read_dir(dest)?.next().is_some() {
        bail!("{} is not empty", dest.display());
    }
    std::fs::create_dir_all(dest)?;
    let repo = Repo::init(dest, importer, vec![])?;
    let mut mirror = GitMirror::open(&repo)?;
    let source_git = git(&["rev-parse", "--absolute-git-dir"], Some(&source), &[])?
        .trim()
        .to_string();
    git(
        &[
            &mirror.gd(),
            "fetch",
            "--quiet",
            &source_git,
            "+HEAD:refs/imported/head",
        ],
        None,
        &[],
    )?;
    let mut count = 0;
    for (sha, author, at, subject) in &commits {
        let work = tree_at(&mirror.dir, sha)?;
        let evidence = vec![Evidence {
            check: "imported from git".into(),
            ok: true,
            detail: sha.clone(),
        }];
        if let Some(lh) = repo.commit_tree(&work, &Actor::person(author), subject, *at, evidence)? {
            count += 1;
            // keep the original id when Grove reproduces the tree exactly,
            // and when its parent kept its id too
            let parent_kept = match repo.landing(&lh)?.parent {
                Some(p) => {
                    mirror.map.contains_key(&p)
                        || (repo.landing(&p)?.parent.is_none()
                            && repo.view_at(&p)?.decls.is_empty())
                }
                None => true,
            };
            if parent_kept
                && mirror.tree_of(&repo.view_at(&lh)?, lh.as_str())?
                    == mirror.commit_tree_of(sha)?
            {
                mirror.map.insert(lh, sha.clone());
            }
        }
    }
    mirror.save()?;
    repo.reset_workspace(DEFAULT_WORKSPACE, importer)?;
    Ok((repo, count))
}

/// A commit's files, read from a git object database.
fn tree_at(git_dir: &Path, sha: &str) -> Result<Work> {
    let gd = format!("--git-dir={}", git_dir.display());
    let listing = git(&[&gd, "ls-tree", "-r", "-z", "--full-tree", sha], None, &[])?;
    let mut entries = vec![];
    for rec in listing.split('\0').filter(|r| !r.is_empty()) {
        let (meta, path) = rec
            .split_once('\t')
            .ok_or_else(|| anyhow!("odd ls-tree line"))?;
        let mut m = meta.split_whitespace();
        let (_mode, kind, blob) = (m.next(), m.next(), m.next());
        if kind == Some("blob") {
            entries.push((path.to_string(), blob.unwrap_or_default().to_string()));
        }
    }
    let mut child = Command::new("git")
        .args([gd.as_str(), "cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    {
        let mut stdin = child.stdin.take().unwrap();
        for (_, blob) in &entries {
            writeln!(stdin, "{blob}")?;
        }
    }
    let out = child.wait_with_output()?;
    let bytes = out.stdout;
    let mut work = Work::new();
    let mut pos = 0;
    for (path, _) in &entries {
        let nl = bytes[pos..]
            .iter()
            .position(|b| *b == b'\n')
            .ok_or_else(|| anyhow!("short cat-file output"))?
            + pos;
        let header = String::from_utf8_lossy(&bytes[pos..nl]).to_string();
        let size: usize = header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("bad cat-file header {header}"))?;
        let body = &bytes[nl + 1..nl + 1 + size];
        pos = nl + 1 + size + 1;
        if ignored_path(path) {
            continue;
        }
        let items = split_bytes(path, body);
        if !items.is_empty() {
            work.insert(path.clone(), items);
        }
    }
    Ok(work)
}

fn ignored_path(path: &str) -> bool {
    path == "zero.graph" || path.ends_with("/zero.graph") || path.starts_with(".zero/")
}

/// Where `pr` put a change.
#[derive(Debug)]
pub struct PullRequest {
    pub branch: String,
    pub commit: String,
    /// The pull request's URL, when the GitHub CLI opened one.
    pub url: Option<String>,
}

/// Owner and repository from a GitHub remote URL.
pub fn github_repo(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    (rest.split('/').count() == 2).then(|| rest.to_string())
}

/// Send a change to a git host for review: a commit on top of its base's
/// commit, pushed as branch `grove/<change>`, and a pull request opened with
/// the GitHub CLI (`gh`) when the remote is on GitHub and `gh` is installed.
pub fn pr(repo: &Repo, remote: &str, change: &Change) -> Result<PullRequest> {
    let (mirror, _) = export(repo)?;
    let gd = mirror.gd();
    let parent = mirror.map.get(&change.base).cloned().ok_or_else(|| {
        anyhow!("the change's base isn't on main's exported history; sync and record it again")
    })?;
    let mut view = repo.view_at(&change.base)?;
    view.apply(&repo.store, &change.ops, &[])
        .map_err(|e| anyhow!(e))?;
    let tree = mirror.tree_of(&view, &change.id)?;
    let mut message = format!(
        "{}\n\nGrove-Change: {} v{}\n",
        change.intent, change.id, change.version
    );
    if change.actor.kind == ActorKind::Agent {
        message.push_str(&format!("Grove-Agent: {}\n", change.actor.name));
    }
    if let Some(p) = &change.actor.for_person {
        message.push_str(&format!("Grove-For: {p}\n"));
    }
    let date = format!("{} +0000", change.created);
    let mut child = Command::new("git")
        .args([
            gd.as_str(),
            "commit-tree",
            tree.as_str(),
            "-p",
            parent.as_str(),
        ])
        .env("GIT_AUTHOR_NAME", &change.actor.name)
        .env("GIT_AUTHOR_EMAIL", email_for(&change.actor))
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_NAME", "Grove")
        .env("GIT_COMMITTER_EMAIL", "grove@agents.grove")
        .env("GIT_COMMITTER_DATE", &date)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(message.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!(
            "git commit-tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let commit = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let branch = format!("grove/{}", change.id);
    git(
        &[
            &gd,
            "push",
            "--quiet",
            "--force",
            remote,
            &format!("{commit}:refs/heads/{branch}"),
        ],
        None,
        &[],
    )?;
    // keep main in step so the pull request's base exists
    git(&[&gd, "push", "--quiet", remote, "main:main"], None, &[])?;

    let url = match github_repo(remote) {
        Some(gh_repo)
            if Command::new("gh")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false) =>
        {
            let body = format!(
                "{}\n\n---\nOpened by Grove from change `{}` v{}, made by {}.",
                change.intent, change.id, change.version, change.actor
            );
            let o = Command::new("gh")
                .args([
                    "pr",
                    "create",
                    "--repo",
                    &gh_repo,
                    "--head",
                    &branch,
                    "--base",
                    "main",
                    "--title",
                    &change.intent,
                    "--body",
                    &body,
                ])
                .output()?;
            if !o.status.success() {
                bail!(
                    "gh pr create failed: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            }
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .last()
                .map(str::to_string)
        }
        _ => None,
    };
    Ok(PullRequest {
        branch,
        commit,
        url,
    })
}

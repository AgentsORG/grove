//! The git bridge.
//!
//! GitHub, GitLab, Cursor, CI systems and every editor speak git, so Grove
//! has to as well. This module turns main's history into ordinary git
//! commits (one per landing, with who, for whom, why and the evidence in
//! trailers), and turns a git repository's history into Grove changes.
//!
//! It drives the `git` command rather than reimplementing git's formats, so
//! what it produces is exactly what git itself would.

use crate::diff::Work;
use crate::hash::Hash;
use crate::model::{Actor, ActorKind, Evidence};
use crate::repo::{mode_word, Repo, DEFAULT_WORKSPACE};
use crate::source::split;
use crate::workdir::materialize;
use anyhow::{anyhow, bail, Context, Result};
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

/// Where the mirror lives and which landing each commit came from.
pub struct GitMirror {
    pub dir: PathBuf,
    map_path: PathBuf,
    pub map: BTreeMap<Hash, String>,
}

impl GitMirror {
    pub fn open(repo: &Repo) -> Result<GitMirror> {
        let dir = repo.dir.join("git");
        let map_path = repo.dir.join("git-map.json");
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
        let map = match std::fs::read(&map_path) {
            Ok(b) => serde_json::from_slice(&b)?,
            Err(_) => BTreeMap::new(),
        };
        Ok(GitMirror { dir, map_path, map })
    }

    fn save(&self) -> Result<()> {
        crate::store::write_atomic(&self.map_path, &serde_json::to_vec_pretty(&self.map)?)
    }
}

/// Write every landing not yet exported as a git commit on `main`, then
/// point `main` at the commit for Grove's current main (so an undo shows up
/// in git as main moving back). Returns the mirror and how many commits were
/// written.
pub fn export(repo: &Repo) -> Result<(GitMirror, usize)> {
    let mut mirror = GitMirror::open(repo)?;
    let main = repo.main()?;
    let history = repo.history(&main, None)?;
    let gd = format!("--git-dir={}", mirror.dir.display());
    let mut written = 0;
    for (h, landing) in history.iter().rev() {
        if mirror.map.contains_key(h) {
            continue;
        }
        let parent = landing
            .parent
            .as_ref()
            .map(|p| {
                mirror
                    .map
                    .get(p)
                    .cloned()
                    .ok_or_else(|| anyhow!("parent {} was not exported", p.short()))
            })
            .transpose()?;
        let view = repo.view_at(h)?;
        let tree_dir =
            std::env::temp_dir().join(crate::model::fresh_id("grove-export-", h.as_str()));
        std::fs::create_dir_all(&tree_dir)?;
        materialize(&tree_dir, &view, None)?;
        let wt = format!("--work-tree={}", tree_dir.display());
        let index = mirror.dir.join(format!("index-{}", std::process::id()));
        let env_index = [("GIT_INDEX_FILE", index.to_string_lossy().to_string())];
        let _ = std::fs::remove_file(&index);
        git(
            &[&gd, &wt, "add", "--all", "."],
            Some(&tree_dir),
            &env_index,
        )?;
        let tree = git(&[&gd, "write-tree"], Some(&tree_dir), &env_index)?
            .trim()
            .to_string();
        let _ = std::fs::remove_file(&index);
        let _ = std::fs::remove_dir_all(&tree_dir);

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
    let head = mirror
        .map
        .get(&main)
        .ok_or_else(|| anyhow!("main was not exported"))?
        .clone();
    git(&[&gd, "update-ref", "refs/heads/main", &head], None, &[])?;
    mirror.save()?;
    Ok((mirror, written))
}

/// Push the mirror's `main` to a git remote (GitHub, GitLab, ...).
pub fn push(repo: &Repo, remote: &str, force: bool) -> Result<String> {
    let (mirror, _) = export(repo)?;
    let gd = format!("--git-dir={}", mirror.dir.display());
    let mut args = vec![gd.as_str(), "push", remote, "main:main"];
    if force {
        args.push("--force-with-lease");
    }
    git(&args, None, &[])?;
    Ok(format!("pushed main to {remote}"))
}

/// Create a Grove repository at `dest` from a git repository's first-parent
/// history: one landed change per commit, keeping each commit's author, date
/// and subject.
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
    let mut count = 0;
    for (sha, author, at, subject) in &commits {
        let work = tree_at(&source, sha)?;
        let evidence = vec![Evidence {
            check: "imported from git".into(),
            ok: true,
            detail: sha.clone(),
        }];
        if repo
            .commit_tree(&work, &Actor::person(author), subject, *at, evidence)?
            .is_some()
        {
            count += 1;
        }
    }
    repo.reset_workspace(DEFAULT_WORKSPACE, importer)?;
    Ok((repo, count))
}

/// A commit's files, read straight from git's object database.
fn tree_at(repo: &Path, sha: &str) -> Result<Work> {
    let listing = git(
        &["ls-tree", "-r", "-z", "--full-tree", sha],
        Some(repo),
        &[],
    )?;
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
        .args(["cat-file", "--batch"])
        .current_dir(repo)
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
        if body.contains(&0) || ignored_path(path) {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(body) {
            let items = split(path, text);
            if !items.is_empty() {
                work.insert(path.clone(), items);
            }
        }
    }
    Ok(work)
}

fn ignored_path(path: &str) -> bool {
    path == "zero.graph" || path.ends_with("/zero.graph") || path.starts_with(".zero/")
}

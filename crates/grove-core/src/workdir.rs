//! Reading and writing working copies.

use crate::diff::Work;
use crate::graph::View;
use crate::source::split;
use anyhow::Result;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Never tracked: Grove's own files, other tools' stores, build output, and
/// Zero's local graph store (`zero.graph` holds machine-local identities;
/// Grove addresses code by its source instead).
const ALWAYS_IGNORED: &[&str] = &[
    ".grove",
    ".grove-workspace",
    ".git",
    "target",
    "node_modules",
    ".zero",
    "zero.graph",
];

pub struct Ignore {
    patterns: Vec<String>,
    skip_dirs: Vec<PathBuf>,
}

impl Ignore {
    /// Read `.groveignore`: one path prefix or `*.ext` per line.
    pub fn load(root: &Path, skip_dirs: Vec<PathBuf>) -> Ignore {
        let patterns = fs::read_to_string(root.join(".groveignore"))
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect();
        Ignore {
            patterns,
            skip_dirs,
        }
    }

    fn ignored(&self, rel: &str, abs: &Path) -> bool {
        if self.skip_dirs.iter().any(|d| abs.starts_with(d)) {
            return true;
        }
        let first = rel.split('/').next().unwrap_or(rel);
        let last = rel.rsplit('/').next().unwrap_or(rel);
        if ALWAYS_IGNORED.contains(&first) || ALWAYS_IGNORED.contains(&last) {
            return true;
        }
        self.patterns.iter().any(|p| match p.strip_prefix("*.") {
            Some(ext) => rel.ends_with(&format!(".{ext}")),
            None => {
                rel == p.trim_end_matches('/')
                    || rel.starts_with(&format!("{}/", p.trim_end_matches('/')))
            }
        })
    }
}

/// Read every tracked text file under `root` into declarations.
pub fn scan(root: &Path, ignore: &Ignore) -> Result<Work> {
    let mut work = Work::new();
    walk(root, root, ignore, &mut work)?;
    Ok(work)
}

fn walk(root: &Path, dir: &Path, ignore: &Ignore, work: &mut Work) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::result::Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let rel = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if ignore.ignored(&rel, &path) {
            continue;
        }
        let ft = e.file_type()?;
        if ft.is_dir() {
            walk(root, &path, ignore, work)?;
        } else if ft.is_file() {
            let bytes = fs::read(&path)?;
            // Phase 1 tracks text. Binary files need chunked storage and locks.
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let items = split(&rel, &text);
            if !items.is_empty() {
                work.insert(rel, items);
            }
        }
    }
    Ok(())
}

/// Write a view into a directory. Files the view no longer has, but which
/// `previous` had, are removed; untracked files are left alone.
pub fn materialize(root: &Path, view: &View, previous: Option<&View>) -> Result<()> {
    let files = view.files();
    if let Some(prev) = previous {
        let now: BTreeSet<&String> = files.keys().collect();
        for old in prev.files().keys() {
            if !now.contains(old) {
                let p = root.join(old);
                if p.exists() {
                    fs::remove_file(&p)?;
                }
            }
        }
    }
    for (rel, text) in files {
        let p = root.join(&rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::read_to_string(&p).ok().as_deref() != Some(text.as_str()) {
            fs::write(&p, text)?;
        }
    }
    Ok(())
}

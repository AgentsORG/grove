//! Exact edges from Zero's own resolver.
//!
//! Grove finds references by name unless it can ask Zero. When the `zero`
//! compiler is installed and the code is a Zero package (it has a
//! `zero.toml`), Grove writes the snapshot to a scratch folder, runs
//! `zero import`, `zero query --refs` and `zero dump`, and maps every
//! resolved reference onto its declarations by file and line. A parameter
//! that happens to share a function's name is then not an edge, and two
//! modules with a function of the same name stay apart.
//!
//! Answers are cached by the snapshot's hash under `~/.grove/cache/edges`
//! (or `$GROVE_HOME/cache/edges`), so Zero runs once for each new state of
//! the program, whichever repository it's in. When Zero isn't installed or
//! the code doesn't compile, that's cached too, and Grove uses names.
//! `GROVE_EDGES=names` turns the resolver off.

use crate::graph::{Edge, EdgeKind, View};
use crate::hash::Hash;
use crate::model::DeclId;
use crate::source::is_zero_file;
use crate::workdir::materialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

type Pairs = Vec<(DeclId, DeclId)>;

/// The `zero` binary: `$GROVE_ZERO`, on PATH, or in `~/.zero/bin`.
pub fn zero_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("GROVE_ZERO") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let exe = if cfg!(windows) { "zero.exe" } else { "zero" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let p = PathBuf::from(home).join(".zero").join("bin").join(exe);
    p.is_file().then_some(p)
}

fn run_json(zero: &Path, dir: &Path, args: &[&str]) -> Option<Value> {
    let out = Command::new(zero)
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Where each declaration sits in its materialized Zero file: lines
/// `[start, end)`, counted from 1. Matches `View::files`, which joins a
/// file's declarations in order with one newline between them.
pub fn line_ranges(view: &View) -> BTreeMap<String, Vec<(usize, usize, DeclId)>> {
    let mut by_file: BTreeMap<String, Vec<(u64, DeclId, &str)>> = BTreeMap::new();
    for (id, n) in &view.decls {
        if is_zero_file(&n.entry.file) {
            by_file.entry(n.entry.file.clone()).or_default().push((
                n.entry.order,
                id.clone(),
                n.source.as_str(),
            ));
        }
    }
    by_file
        .into_iter()
        .map(|(f, mut items)| {
            items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
            let mut line = 1;
            let ranges = items
                .into_iter()
                .map(|(_, id, src)| {
                    // the joining newline ends a declaration without one
                    let n = src.matches('\n').count() + usize::from(!src.ends_with('\n'));
                    let r = (line, line + n.max(1), id);
                    line += n + usize::from(src.ends_with('\n'));
                    r
                })
                .collect();
            (f, ranges)
        })
        .collect()
}

fn ask_zero(view: &View, zero: &Path, salt: &str) -> Option<Pairs> {
    let dir = std::env::temp_dir().join(crate::model::fresh_id("grove-zero-", salt));
    std::fs::create_dir_all(&dir).ok()?;
    let result = (|| -> Option<Pairs> {
        materialize(&dir, view, None).ok()?;
        let import = Command::new(zero)
            .args(["import", "."])
            .current_dir(&dir)
            .output()
            .ok()?;
        if !import.status.success() {
            return None;
        }
        let refs = run_json(zero, &dir, &["query", "--json", "--refs", ".", "."])?;
        let dump = run_json(zero, &dir, &["dump", "--json", "."])?;
        let mut place: HashMap<String, (String, usize)> = HashMap::new();
        for n in dump.get("nodes")?.as_array()? {
            let id = n.get("id").and_then(Value::as_str);
            let path = n.get("path").and_then(Value::as_str);
            let line = n.get("line").and_then(Value::as_u64);
            if let (Some(id), Some(path), Some(line)) = (id, path, line) {
                place.insert(id.to_string(), (path.replace('\\', "/"), line as usize));
            }
        }
        let ranges = line_ranges(view);
        let at = |path: &str, line: usize| -> Option<DeclId> {
            ranges
                .get(path)?
                .iter()
                .find(|(a, b, _)| *a <= line && line < *b)
                .map(|(_, _, id)| id.clone())
        };
        let locate = |node: &str| place.get(node).and_then(|(p, l)| at(p, *l));
        let mut pairs = BTreeSet::new();
        for r in refs.get("references")?.as_array()? {
            if r.get("resolved").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            let kind = r.get("targetKind").and_then(Value::as_str).unwrap_or("");
            if matches!(
                kind,
                "param" | "local" | "member" | "field" | "variable" | "binding"
            ) {
                continue;
            }
            let from = r
                .get("node")
                .and_then(Value::as_str)
                .and_then(locate)
                .or_else(|| {
                    let path = r.get("path").and_then(Value::as_str)?.replace('\\', "/");
                    let line = r.get("line").and_then(Value::as_u64)? as usize;
                    at(&path, line)
                });
            let to = r.get("targetNode").and_then(Value::as_str).and_then(locate);
            if let (Some(from), Some(to)) = (from, to) {
                if from != to {
                    pairs.insert((from, to));
                }
            }
        }
        Some(pairs.into_iter().collect())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn memo() -> &'static Mutex<HashMap<String, Option<Pairs>>> {
    static M: OnceLock<Mutex<HashMap<String, Option<Pairs>>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// Exact code edges for a view, or `None` to fall back to names.
pub fn resolved_edges(view: &View) -> Option<Vec<Edge>> {
    if std::env::var("GROVE_EDGES").is_ok_and(|v| v == "names") {
        return None;
    }
    let files: BTreeSet<&str> = view.decls.values().map(|n| n.entry.file.as_str()).collect();
    if !files.contains("zero.toml") || !files.iter().any(|f| is_zero_file(f)) {
        return None;
    }
    let key = Hash::of("edges-v1", &serde_json::to_vec(&view.snapshot()).ok()?);
    let key = key.hex().to_string();
    if let Some(hit) = memo().lock().ok()?.get(&key) {
        return hit.as_ref().map(to_edges);
    }
    let cache = crate::trust::grove_home()
        .join("cache")
        .join("edges")
        .join(format!("{key}.json"));
    let pairs: Option<Pairs> = match std::fs::read(&cache)
        .ok()
        .and_then(|b| serde_json::from_slice::<Option<Pairs>>(&b).ok())
    {
        Some(cached) => cached,
        None => {
            // no zero: don't cache, it may be installed later
            let zero = zero_bin()?;
            let answer = ask_zero(view, &zero, &key);
            if let Ok(bytes) = serde_json::to_vec(&answer) {
                let _ = std::fs::create_dir_all(cache.parent()?);
                let _ = crate::store::write_atomic(&cache, &bytes);
            }
            answer
        }
    };
    let edges = pairs.as_ref().map(to_edges);
    if let Ok(mut m) = memo().lock() {
        m.insert(key, pairs);
    }
    edges
}

/// Where a view's code edges come from: `"zero"` or `"names"`.
pub fn edge_source(view: &View) -> &'static str {
    if resolved_edges(view).is_some() {
        "zero"
    } else {
        "names"
    }
}

fn to_edges(pairs: &Pairs) -> Vec<Edge> {
    pairs
        .iter()
        .map(|(from, to)| Edge {
            from: from.clone(),
            to: to.clone(),
            kind: EdgeKind::Code,
        })
        .collect()
}

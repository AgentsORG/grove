//! Checks: the evidence a landing carries.
//!
//! Before anything reaches main, the combined result (main plus the change,
//! replayed) is written to a scratch directory and every configured check
//! runs there. If one fails, nothing lands. This is the merge queue: it
//! judges the result, not the change alone.

use crate::graph::View;
use crate::model::Evidence;
use crate::workdir::materialize;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckSpec {
    pub name: String,
    /// A shell command, run in the scratch copy of the result.
    pub run: String,
}

/// Run every check against `view`. Returns the evidence, failing or not.
pub fn run_checks(view: &View, checks: &[CheckSpec]) -> Result<Vec<Evidence>> {
    let mut evidence = vec![Evidence {
        check: "grove: graph".into(),
        ok: true,
        detail: graph_detail(view),
    }];
    if checks.is_empty() {
        return Ok(evidence);
    }
    let dir = tempfile_dir()?;
    materialize(&dir, view, None)?;
    for c in checks {
        let out = shell(&c.run, &dir);
        let (ok, detail) = match out {
            Ok(o) => {
                let mut text = String::from_utf8_lossy(&o.stdout).to_string();
                text.push_str(&String::from_utf8_lossy(&o.stderr));
                (o.status.success(), tail(&text, 12))
            }
            Err(e) => (false, format!("could not run: {e}")),
        };
        evidence.push(Evidence {
            check: c.name.clone(),
            ok,
            detail,
        });
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(evidence)
}

fn graph_detail(view: &View) -> String {
    let clashes = view.name_clashes();
    if clashes.is_empty() {
        format!(
            "{} declarations, {} edges (from {})",
            view.decls.len(),
            view.edges().len(),
            crate::zero::edge_source(view)
        )
    } else {
        format!(
            "{} declarations; same name twice: {}",
            view.decls.len(),
            clashes.join(", ")
        )
    }
}

fn shell(cmd: &str, dir: &Path) -> std::io::Result<std::process::Output> {
    crate::shell::run(cmd, dir)
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.trim_end().lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn tempfile_dir() -> Result<std::path::PathBuf> {
    let d = std::env::temp_dir().join(crate::model::fresh_id("grove-check-", "dir"));
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

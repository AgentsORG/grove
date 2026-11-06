//! Grove and git, both ways. Skipped when git isn't installed.

use grove_core::*;
use std::fs;
use std::process::Command;

fn has_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn export_writes_one_commit_per_landing_with_trailers() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.0"), "fn a() -> u32 {\n    return 1\n}\n").unwrap();
    let me = Actor::person("meera");
    let repo = Repo::init(&root, &me, vec![]).unwrap();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let ws = repo
        .add_workspace("codex", &tmp.path().join("codex"), &codex)
        .unwrap();
    fs::write(ws.join("a.0"), "fn a() -> u32 {\n    return 2\n}\n").unwrap();
    repo.record("codex", "Return two", &codex).unwrap();
    repo.land("codex", None, &codex).unwrap();

    let (mirror, written) = git::export(&repo).unwrap();
    assert_eq!(written, 2);
    let log = git(&mirror.dir, &["log", "--format=%an|%ae|%s", "main"]);
    assert_eq!(
        log.lines().next().unwrap(),
        "codex|codex@agents.grove|Return two"
    );
    let body = git(&mirror.dir, &["log", "-1", "--format=%B", "main"]);
    assert!(
        body.contains("Grove-Agent: codex") && body.contains("Grove-For: rishitabh"),
        "{body}"
    );
    assert_eq!(
        git(&mirror.dir, &["show", "main:a.0"]),
        "fn a() -> u32 {\n    return 2\n}\n"
    );

    // exporting again writes nothing new; an undo moves git's main back
    assert_eq!(git::export(&repo).unwrap().1, 0);
    repo.undo(&me).unwrap();
    let (mirror, _) = git::export(&repo).unwrap();
    assert_eq!(
        git(&mirror.dir, &["log", "--format=%s", "main"])
            .lines()
            .count(),
        1
    );
}

#[test]
fn import_turns_git_history_into_changes() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src-repo");
    fs::create_dir_all(&src).unwrap();
    git(&src, &["init", "--quiet", "--initial-branch=main"]);
    let commit = |who: &str, msg: &str| {
        git(&src, &["add", "--all"]);
        git(
            &src,
            &[
                "-c",
                &format!("user.name={who}"),
                "-c",
                "user.email=x@y",
                "commit",
                "--quiet",
                "-m",
                msg,
            ],
        );
    };
    fs::write(src.join("main.0"), "fn a() -> u32 {\n    return 1\n}\n").unwrap();
    fs::write(src.join("README.md"), "# demo\n").unwrap();
    commit("Harshit", "First");
    fs::write(
        src.join("main.0"),
        "fn a() -> u32 {\n    return 1\n}\n\nfn b() -> u32 {\n    return a()\n}\n",
    )
    .unwrap();
    commit("Meera", "Add b");

    let dest = tmp.path().join("grove-repo");
    let (repo, n) = git::import(&src, &dest, &Actor::person("importer"), None).unwrap();
    assert_eq!(n, 2);
    let history = repo.history(&repo.main().unwrap(), None).unwrap();
    let last = repo.change(history[0].1.change.as_ref().unwrap()).unwrap();
    assert_eq!(
        (last.actor.name.as_str(), last.intent.as_str()),
        ("Meera", "Add b")
    );
    let view = repo.main_view().unwrap();
    assert!(view
        .edges()
        .iter()
        .any(|e| view.decls[&e.from].entry.name == "b" && view.decls[&e.to].entry.name == "a"));
    // the working copy holds main
    assert!(fs::read_to_string(dest.join("main.0"))
        .unwrap()
        .contains("fn b()"));
    // and the history round-trips back out to git
    let (mirror, written) = git::export(&repo).unwrap();
    assert_eq!(written, 3);
    assert_eq!(git(&mirror.dir, &["show", "main:README.md"]), "# demo\n");
}

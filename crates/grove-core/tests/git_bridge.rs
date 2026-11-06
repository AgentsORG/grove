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
    // and the history round-trips back out to git with the very same commits
    let (mirror, written) = git::export(&repo).unwrap();
    assert_eq!(written, 0, "every imported commit kept its id");
    assert_eq!(
        git(&mirror.dir, &["rev-parse", "main"]),
        git(&src, &["rev-parse", "HEAD"])
    );
    assert_eq!(git(&mirror.dir, &["show", "main:README.md"]), "# demo\n");
}

#[test]
fn binary_files_survive_grove_and_git() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    let png: Vec<u8> = (0u8..=255).chain([0, 0, 137, 80, 78, 71]).collect();
    fs::write(root.join("logo.png"), &png).unwrap();
    fs::write(root.join("notes.txt"), "no final newline\r\nkept as is").unwrap();
    let me = Actor::person("meera");
    let repo = Repo::init(&root, &me, vec![]).unwrap();
    assert!(
        repo.status(DEFAULT_WORKSPACE).unwrap().is_empty(),
        "a binary file reads back unchanged"
    );
    let view = repo.main_view().unwrap();
    assert_eq!(view.file_bytes()["logo.png"], png);
    assert_eq!(view.files()["notes.txt"], "no final newline\r\nkept as is");
    let (mirror, _) = git::export(&repo).unwrap();
    let out = Command::new("git")
        .args(["show", "main:logo.png"])
        .current_dir(&mirror.dir)
        .output()
        .unwrap();
    assert_eq!(out.stdout, png);
}

#[test]
fn pull_brings_in_commits_made_on_the_git_side() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("a.0"),
        "fn a() -> u32 {\n    return 1\n}\n\nfn b() -> u32 {\n    return 2\n}\n",
    )
    .unwrap();
    let me = Actor::person("meera");
    let repo = Repo::init(&root, &me, vec![]).unwrap();
    // publish to a bare "GitHub"
    let hub = tmp.path().join("hub.git");
    git(
        tmp.path(),
        &[
            "init",
            "--bare",
            "--quiet",
            "--initial-branch=main",
            hub.to_str().unwrap(),
        ],
    );
    git::push(&repo, hub.to_str().unwrap(), false).unwrap();

    // someone commits on GitHub
    let clone = tmp.path().join("clone");
    git(
        tmp.path(),
        &[
            "clone",
            "--quiet",
            hub.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let text = fs::read_to_string(clone.join("a.0")).unwrap();
    fs::write(clone.join("a.0"), text.replace("return 2", "return 20")).unwrap();
    git(
        &clone,
        &[
            "-c",
            "user.name=Dev",
            "-c",
            "user.email=d@x",
            "commit",
            "--quiet",
            "-am",
            "Twenty",
        ],
    );
    git(&clone, &["push", "--quiet", "origin", "main"]);

    // meanwhile Grove's main moves too, on another declaration
    edit_and_land(&repo, &root, "return 1", "return 10", "Ten");

    let pulled = git::pull(&repo, hub.to_str().unwrap(), &me).unwrap();
    assert_eq!((pulled.replayed, pulled.conflict.is_none()), (1, true));
    let main = repo.main_view().unwrap().files()["a.0"].clone();
    assert!(
        main.contains("return 10") && main.contains("return 20"),
        "{main}"
    );
    // and pushing again fast-forwards: the git commit is merged, not lost
    git::push(&repo, hub.to_str().unwrap(), false).unwrap();
    let log = git(&hub, &["log", "--format=%s", "main"]);
    assert!(log.contains("Twenty") && log.contains("Ten"), "{log}");
}

fn edit_and_land(repo: &Repo, root: &std::path::Path, from: &str, to: &str, intent: &str) {
    let p = root.join("a.0");
    let text = fs::read_to_string(&p).unwrap();
    fs::write(&p, text.replacen(from, to, 1)).unwrap();
    let me = Actor::person("meera");
    repo.record(DEFAULT_WORKSPACE, intent, &me).unwrap();
    repo.land(DEFAULT_WORKSPACE, None, &me).unwrap();
}

#[test]
fn branches_import_as_lanes_and_export_as_branches() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src-repo");
    fs::create_dir_all(&src).unwrap();
    git(&src, &["init", "--quiet", "--initial-branch=main"]);
    let commit = |msg: &str| {
        git(&src, &["add", "--all"]);
        let args = [
            "-c",
            "user.name=meera",
            "-c",
            "user.email=x@y",
            "commit",
            "--quiet",
            "-m",
            msg,
        ];
        git(&src, &args);
        git(&src, &["rev-parse", "HEAD"]).trim().to_string()
    };
    fs::write(src.join("a.0"), "fn a() -> u32 {\n    return 1\n}\n").unwrap();
    commit("Start");
    // a branch that was merged: nothing of its own
    git(&src, &["branch", "done"]);
    // a branch with two commits of its own
    git(&src, &["checkout", "--quiet", "-b", "feature/b"]);
    fs::write(src.join("b.0"), "fn b() -> u32 {\n    return 2\n}\n").unwrap();
    commit("Add b");
    fs::write(src.join("b.0"), "fn b() -> u32 {\n    return 3\n}\n").unwrap();
    let b_tip = commit("Three");
    // main moves on
    git(&src, &["checkout", "--quiet", "main"]);
    fs::write(src.join("a.0"), "fn a() -> u32 {\n    return 9\n}\n").unwrap();
    let main_tip = commit("Nine");

    let dest = tmp.path().join("grove");
    let (repo, n) = git::import(&src, &dest, &Actor::person("meera"), None).unwrap();
    assert_eq!(n, 4, "two on main, two on the lane");
    let refs = repo.refs().unwrap();
    assert_eq!(refs.lanes.keys().collect::<Vec<_>>(), vec!["feature/b"]);
    let lane = repo.view_at(&refs.lanes["feature/b"]).unwrap().files();
    assert!(lane["b.0"].contains("return 3") && lane["a.0"].contains("return 1"));
    assert!(!repo.main_view().unwrap().files().contains_key("b.0"));

    // back out to git: same branches, same commit ids
    let (mirror, _) = git::export(&repo).unwrap();
    assert_eq!(git(&mirror.dir, &["rev-parse", "main"]).trim(), main_tip);
    assert_eq!(git(&mirror.dir, &["rev-parse", "feature/b"]).trim(), b_tip);

    // a lane started in Grove exports as a branch too
    repo.lane_new("spike", None, &Actor::person("meera"))
        .unwrap();
    let (mirror, _) = git::export(&repo).unwrap();
    assert_eq!(git(&mirror.dir, &["rev-parse", "spike"]).trim(), main_tip);
}

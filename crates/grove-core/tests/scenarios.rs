//! The stories from the Grove deck, run end to end on real folders.

use grove_core::*;
use std::fs;
use std::path::{Path, PathBuf};

const API: &str = r#"use std.mem

const base: u32 = 100

pub fn main(world: World) -> Void raises {
    let h: Header = parse(read())
    check world.out.write("ok\n")
}

fn parse(b: Span<u8>) -> Header {
    return firstLine(b)
}

fn handle(request: Span<u8>) -> Void {
    let n: u32 = fetchAll(3)
}

fn retryDelay(attempt: u32) -> u32 {
    return base * attempt
}

fn fetchAll(times: u32) -> u32 {
    return retryDelay(times)
}

fn rateLimit(key: u32) -> u32 {
    return key
}

fn checkoutLabel() -> u32 {
    return 1
}
"#;

const DOCS: &str = "# API\n\nCall `rateLimit` before `handle`.\n";

struct Team {
    _tmp: tempfile::TempDir,
    repo: Repo,
    root: PathBuf,
    harshit: Actor,
}

impl Team {
    fn new() -> Team {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("acme-api");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/main.0"), API).unwrap();
        fs::write(root.join("docs/api.md"), DOCS).unwrap();
        let harshit = Actor::person("harshit");
        let repo = Repo::init(&root, &harshit, vec![]).unwrap();
        Team {
            _tmp: tmp,
            repo,
            root,
            harshit,
        }
    }

    /// A new workspace for someone, next to the repo.
    fn workspace(&self, name: &str, actor: &Actor) -> PathBuf {
        let path = self.root.parent().unwrap().join(name);
        self.repo.add_workspace(name, &path, actor).unwrap()
    }

    fn main_file(&self, rel: &str) -> String {
        self.repo.main_view().unwrap().files()[rel].clone()
    }

    fn land(&self, ws: &str, actor: &Actor) -> LandOutcome {
        self.repo.land(ws, None, actor).unwrap()
    }
}

fn edit(dir: &Path, rel: &str, from: &str, to: &str) {
    let p = dir.join(rel);
    let text = fs::read_to_string(&p).unwrap();
    assert!(text.contains(from), "{from:?} not found in {rel}");
    fs::write(&p, text.replacen(from, to, 1)).unwrap();
}

fn landed_mode(o: &LandOutcome) -> LandMode {
    match o {
        LandOutcome::Landed { mode, .. } => mode.clone(),
        LandOutcome::Rejected { evidence, .. } => panic!("rejected: {evidence:?}"),
        LandOutcome::Conflict { conflict, .. } => panic!("conflict: {conflict:?}"),
    }
}

#[test]
fn init_splits_the_program_into_a_graph() {
    let t = Team::new();
    let view = t.repo.main_view().unwrap();
    let names: Vec<_> = view.decls.values().map(|n| n.entry.name.as_str()).collect();
    for n in [
        "main",
        "parse",
        "retryDelay",
        "fetchAll",
        "docs/api.md",
        "use std.mem",
        "base",
    ] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    let edges = view.edges();
    let id = |n: &str| view.find(n).unwrap();
    assert!(edges
        .iter()
        .any(|e| e.from == id("fetchAll") && e.to == id("retryDelay") && e.kind == EdgeKind::Code));
    assert!(edges.iter().any(|e| e.from == id("docs/api.md")
        && e.to == id("rateLimit")
        && e.kind == EdgeKind::Mention));
    // the files come back exactly
    assert_eq!(t.main_file("src/main.0"), API);
    assert_eq!(t.main_file("docs/api.md"), DOCS);
}

#[test]
fn separate_work_lands_together() {
    let t = Team::new();
    let claude = Actor::agent("claude-code", Some("harshit"));
    let meera = Actor::person("meera");
    let a = t.workspace("claude", &claude);
    let b = t.workspace("meera", &meera);

    edit(&a, "src/main.0", "return key", "return key % 1000");
    edit(&b, "src/main.0", "return 1", "return 2");
    t.repo
        .record("claude", "Per-key rate limit", &claude)
        .unwrap();
    t.repo
        .record("meera", "Checkout button copy", &meera)
        .unwrap();

    assert_eq!(landed_mode(&t.land("claude", &claude)), LandMode::Direct);
    match landed_mode(&t.land("meera", &meera)) {
        LandMode::Together { with } => assert_eq!(with.len(), 1),
        m => panic!("expected together, got {m:?}"),
    }
    let main = t.main_file("src/main.0");
    assert!(main.contains("return key % 1000") && main.contains("return 2"));
}

#[test]
fn work_that_reads_a_changed_node_is_ordered_after_it() {
    let t = Team::new();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let cursor = Actor::agent("cursor", Some("dev"));
    let a = t.workspace("codex", &codex);
    let b = t.workspace("cursor", &cursor);

    edit(
        &a,
        "src/main.0",
        "return base * attempt",
        "return base * pow2(attempt)",
    );
    edit(
        &b,
        "src/main.0",
        "return retryDelay(times)",
        "return retryDelay(times) + 1",
    );
    t.repo
        .record("codex", "Back off retries exponentially", &codex)
        .unwrap();
    t.repo
        .record("cursor", "Use retryDelay in fetchAll", &cursor)
        .unwrap();

    t.land("codex", &codex);
    match landed_mode(&t.land("cursor", &cursor)) {
        LandMode::Ordered { after } => assert_eq!(after.len(), 1),
        m => panic!("expected ordered, got {m:?}"),
    }
}

#[test]
fn a_rename_reaches_call_sites_added_after_it_was_written() {
    let t = Team::new();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let dev = Actor::person("dev");
    let a = t.workspace("codex", &codex);
    let b = t.workspace("dev", &dev);

    // Codex renames parse everywhere it can see it
    let text = fs::read_to_string(a.join("src/main.0")).unwrap();
    fs::write(a.join("src/main.0"), text.replace("parse(", "parseHeader(")).unwrap();
    let change = t
        .repo
        .record("codex", "Rename parse to parseHeader", &codex)
        .unwrap();
    assert!(
        matches!(change.ops[0], Op::Rename { .. }),
        "{:?}",
        change.ops
    );
    assert_eq!(
        change.ops.len(),
        1,
        "the call site in main is carried by the rename: {:?}",
        change.ops
    );

    // Dev, working from the old main, adds a new call to parse
    edit(
        &b,
        "src/main.0",
        "let n: u32 = fetchAll(3)",
        "let n: u32 = fetchAll(3)\n    let h: Header = parse(request)",
    );
    t.repo
        .record("dev", "Parse the request in handle", &dev)
        .unwrap();

    // Dev lands first; the rename then lands and rewrites Dev's new call
    t.land("dev", &dev);
    t.land("codex", &codex);
    let main = t.main_file("src/main.0");
    assert!(main.contains("parseHeader(request)"), "{main}");
    assert!(!main.contains("parse("), "{main}");

    // and the other order works too: the rename is carried into later work
    let t = Team::new();
    let a = t.workspace("codex", &codex);
    let b = t.workspace("dev", &dev);
    let text = fs::read_to_string(a.join("src/main.0")).unwrap();
    fs::write(a.join("src/main.0"), text.replace("parse(", "parseHeader(")).unwrap();
    t.repo
        .record("codex", "Rename parse to parseHeader", &codex)
        .unwrap();
    edit(
        &b,
        "src/main.0",
        "let n: u32 = fetchAll(3)",
        "let n: u32 = fetchAll(3)\n    let h: Header = parse(request)",
    );
    t.repo
        .record("dev", "Parse the request in handle", &dev)
        .unwrap();
    t.land("codex", &codex);
    t.land("dev", &dev);
    let main = t.main_file("src/main.0");
    assert!(
        main.contains("parseHeader(request)") && !main.contains("parse("),
        "{main}"
    );
}

#[test]
fn the_same_declaration_changed_twice_is_a_saved_conflict() {
    let t = Team::new();
    let dev = Actor::person("dev");
    let cursor = Actor::agent("cursor", Some("harshit"));
    let a = t.workspace("dev", &dev);
    let b = t.workspace("cursor", &cursor);
    edit(
        &a,
        "src/main.0",
        "return base * attempt",
        "return min(base * attempt, 5000)",
    );
    edit(
        &b,
        "src/main.0",
        "return base * attempt",
        "return base * pow2(attempt)",
    );
    t.repo.record("dev", "Cap retries", &dev).unwrap();
    t.repo
        .record("cursor", "Exponential retries", &cursor)
        .unwrap();
    t.land("dev", &dev);
    match t.land("cursor", &cursor) {
        LandOutcome::Conflict { conflict, .. } => {
            assert_eq!(conflict.decls[0].name, "retryDelay");
            assert!(conflict.decls[0].main.as_ref().unwrap().contains("min("));
            assert!(conflict.decls[0].mine.as_ref().unwrap().contains("pow2("));
        }
        o => panic!("expected a conflict, got {o:?}"),
    }
    // main kept Dev's version; the conflict is saved for anyone to pick up
    assert!(t.main_file("src/main.0").contains("min("));
    assert_eq!(t.repo.refs().unwrap().conflicts.len(), 1);
}

#[test]
fn identical_edits_are_not_a_conflict() {
    let t = Team::new();
    let a_actor = Actor::person("a");
    let b_actor = Actor::person("b");
    let a = t.workspace("a", &a_actor);
    let b = t.workspace("b", &b_actor);
    edit(&a, "src/main.0", "return 1", "return 3");
    edit(&b, "src/main.0", "return 1", "return 3");
    t.repo.record("a", "same", &a_actor).unwrap();
    t.repo.record("b", "same", &b_actor).unwrap();
    t.land("a", &a_actor);
    landed_mode(&t.land("b", &b_actor));
}

#[test]
fn a_failing_check_keeps_main_where_it_was() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.0"), "fn a() -> u32 {\n    return 1\n}\n").unwrap();
    let me = Actor::person("me");
    let repo = Repo::init(
        &root,
        &me,
        vec![CheckSpec {
            name: "always fails".into(),
            run: "exit 1".into(),
        }],
    )
    .unwrap();
    let before = repo.main().unwrap();
    edit(&root, "a.0", "return 1", "return 2");
    repo.record(DEFAULT_WORKSPACE, "two", &me).unwrap();
    match repo.land(DEFAULT_WORKSPACE, None, &me).unwrap() {
        LandOutcome::Rejected { evidence, .. } => assert!(evidence.iter().any(|e| !e.ok)),
        o => panic!("expected rejection, got {o:?}"),
    }
    assert_eq!(repo.main().unwrap(), before);
}

#[test]
fn undo_puts_main_back_and_the_change_back_in_drafts() {
    let t = Team::new();
    edit(&t.root, "src/main.0", "return 1", "return 9");
    let c = t
        .repo
        .record(DEFAULT_WORKSPACE, "nine", &t.harshit)
        .unwrap();
    let before = t.repo.main().unwrap();
    t.land(DEFAULT_WORKSPACE, &t.harshit);
    assert_ne!(t.repo.main().unwrap(), before);
    let msg = t.repo.undo(&t.harshit).unwrap();
    assert!(msg.contains("landed"), "{msg}");
    assert_eq!(t.repo.main().unwrap(), before);
    assert!(t.repo.refs().unwrap().drafts.contains_key(&c.id));
}

#[test]
fn why_follows_a_declaration_through_its_rename() {
    let t = Team::new();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let a = t.workspace("codex", &codex);
    edit(
        &a,
        "src/main.0",
        "return firstLine(b)",
        "return firstLine(b, 1)",
    );
    t.repo.record("codex", "Read one line", &codex).unwrap();
    t.land("codex", &codex);
    let text = fs::read_to_string(a.join("src/main.0")).unwrap();
    fs::write(a.join("src/main.0"), text.replace("parse(", "parseHeader(")).unwrap();
    t.repo
        .record("codex", "Rename parse to parseHeader", &codex)
        .unwrap();
    t.land("codex", &codex);

    let (_, hits) = t.repo.why("parseHeader", true).unwrap();
    let intents: Vec<_> = hits.iter().map(|(_, c)| c.intent.as_str()).collect();
    assert_eq!(
        intents,
        vec!["Rename parse to parseHeader", "Read one line"]
    );
    assert_eq!(hits[0].1.actor.to_string(), "codex for rishitabh");
}

#[test]
fn a_workspace_syncs_past_others_and_keeps_its_own_edits() {
    let t = Team::new();
    let meera = Actor::person("meera");
    let dev = Actor::person("dev");
    let a = t.workspace("meera", &meera);
    let b = t.workspace("dev", &dev);
    edit(&a, "src/main.0", "return 1", "return 2");
    t.repo.record("meera", "two", &meera).unwrap();
    t.land("meera", &meera);
    // Dev has an unrecorded edit elsewhere
    edit(&b, "src/main.0", "return key", "return key + 1");
    t.repo.sync("dev", &dev).unwrap();
    let text = fs::read_to_string(b.join("src/main.0")).unwrap();
    assert!(
        text.contains("return 2") && text.contains("return key + 1"),
        "{text}"
    );
}

#[test]
fn recording_again_makes_a_new_version_of_the_same_change() {
    let t = Team::new();
    edit(&t.root, "src/main.0", "return 1", "return 2");
    let v1 = t.repo.record(DEFAULT_WORKSPACE, "two", &t.harshit).unwrap();
    edit(&t.root, "src/main.0", "return 2", "return 3");
    let v2 = t
        .repo
        .record(DEFAULT_WORKSPACE, "three", &t.harshit)
        .unwrap();
    assert_eq!(v1.id, v2.id);
    assert_eq!(v2.version, 2);
}

#[test]
fn the_same_code_is_the_same_graph_on_every_machine() {
    // Two machines, two folders, two people, no shared state: the program
    // graph and every address in it come out identical.
    let one = Team::new();
    let two = Team::new();
    let snap = |t: &Team| t.repo.landing(&t.repo.main().unwrap()).unwrap().snapshot;
    assert_eq!(snap(&one), snap(&two));
    assert_eq!(
        one.repo
            .main_view()
            .unwrap()
            .decls
            .keys()
            .collect::<Vec<_>>(),
        two.repo
            .main_view()
            .unwrap()
            .decls
            .keys()
            .collect::<Vec<_>>()
    );

    // and the same edit made on each machine produces the same result
    edit(&one.root, "src/main.0", "return 1", "return 5");
    edit(&two.root, "src/main.0", "return 1", "return 5");
    for t in [&one, &two] {
        t.repo
            .record(DEFAULT_WORKSPACE, "five", &t.harshit)
            .unwrap();
        t.land(DEFAULT_WORKSPACE, &t.harshit);
    }
    assert_eq!(snap(&one), snap(&two));
}

#[test]
fn two_people_adding_the_same_function_is_caught() {
    let t = Team::new();
    let meera = Actor::person("meera");
    let codex = Actor::agent("codex", Some("dev"));
    let a = t.workspace("meera", &meera);
    let b = t.workspace("codex", &codex);
    let add = |dir: &Path, body: &str| {
        let p = dir.join("src/main.0");
        let text = fs::read_to_string(&p).unwrap();
        fs::write(
            &p,
            format!("{text}\nfn clamp(x: u32) -> u32 {{\n    return {body}\n}}\n"),
        )
        .unwrap();
    };
    add(&a, "x");
    add(&b, "min(x, 10)");
    t.repo.record("meera", "Add clamp", &meera).unwrap();
    t.repo.record("codex", "Add clamp too", &codex).unwrap();
    t.land("meera", &meera);
    match t.land("codex", &codex) {
        LandOutcome::Conflict { conflict, .. } => assert_eq!(conflict.decls[0].name, "clamp"),
        o => panic!("two different clamps must not both land: {o:?}"),
    }
}

#[test]
fn the_store_verifies() {
    let t = Team::new();
    let (n, bad) = t.repo.verify().unwrap();
    assert!(n > 0);
    assert!(bad.is_empty(), "{bad:?}");
}

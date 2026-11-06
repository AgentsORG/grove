//! Working through a remote: a hub (a bare repository, as GroveHub hosts
//! them) and people and agents with their own clones of it.

use grove_core::*;
use std::fs;
use std::path::{Path, PathBuf};

const API: &str = r#"const base: u32 = 100

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

struct World {
    tmp: tempfile::TempDir,
    hub: PathBuf,
    harshit: Actor,
}

impl World {
    /// Harshit starts a repository on his laptop and pushes it to a new,
    /// empty hub.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let laptop = tmp.path().join("laptop");
        fs::create_dir_all(laptop.join("src")).unwrap();
        fs::write(laptop.join("src/main.0"), API).unwrap();
        fs::write(laptop.join("README.md"), "# acme\n\nCall `rateLimit`.\n").unwrap();
        let harshit = Actor::person("harshit");
        let repo = Repo::init(&laptop, &harshit, vec![]).unwrap();
        let hub = tmp.path().join("hub");
        Repo::init_bare(&hub, &harshit).unwrap();
        repo.set_remote("origin", &hub.to_string_lossy()).unwrap();
        let report = repo
            .push_to(
                DEFAULT_WORKSPACE,
                "origin",
                &folder(&hub),
                None,
                false,
                &harshit,
            )
            .unwrap();
        assert_eq!(report.landings, 1);
        World { tmp, hub, harshit }
    }

    fn clone(&self, name: &str, actor: &Actor) -> (Repo, PathBuf) {
        let dest = self.tmp.path().join(name);
        let repo = Repo::clone_from(
            &self.hub.to_string_lossy(),
            &folder(&self.hub),
            &dest,
            actor,
        )
        .unwrap();
        (repo, dest)
    }

    fn hub(&self) -> Repo {
        Repo::open(&self.hub).unwrap()
    }
}

fn folder(p: &Path) -> Folder {
    Folder::open(p).unwrap()
}

fn edit(dir: &Path, from: &str, to: &str) {
    let p = dir.join("src/main.0");
    let text = fs::read_to_string(&p).unwrap();
    assert!(text.contains(from), "{from:?} not found");
    fs::write(&p, text.replacen(from, to, 1)).unwrap();
}

/// Record the workspace's edits and push them to land on the hub.
fn push(w: &World, repo: &Repo, intent: &str, actor: &Actor, review: bool) -> PushReport {
    repo.record(DEFAULT_WORKSPACE, intent, actor).unwrap();
    repo.push_to(
        DEFAULT_WORKSPACE,
        "origin",
        &folder(&w.hub),
        None,
        review,
        actor,
    )
    .unwrap()
}

fn mode(r: &PushReport) -> LandMode {
    match r.proposal.as_ref().unwrap() {
        Proposal::Landed { mode, .. } => mode.clone(),
        other => panic!("did not land: {other:?}"),
    }
}

#[test]
fn a_clone_holds_the_same_graph_and_files() {
    let w = World::new();
    let (repo, dest) = w.clone("meera", &Actor::person("meera"));
    assert_eq!(repo.main().unwrap(), w.hub().main().unwrap());
    assert_eq!(fs::read_to_string(dest.join("src/main.0")).unwrap(), API);
    assert_eq!(repo.main_view().unwrap(), w.hub().main_view().unwrap());
    assert_eq!(
        repo.config().unwrap().remotes["origin"],
        w.hub.to_string_lossy()
    );
    let (n, bad) = repo.verify().unwrap();
    assert!(n > 0 && bad.is_empty());
}

#[test]
fn agents_on_separate_clones_land_together_on_the_hub() {
    let w = World::new();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let claude = Actor::agent("claude-code", Some("harshit"));
    let (a, a_dir) = w.clone("codex", &codex);
    let (b, b_dir) = w.clone("claude", &claude);

    edit(&a_dir, "return key", "return key % 64");
    edit(&b_dir, "return 1", "return 2");
    assert_eq!(
        mode(&push(&w, &a, "Per-key rate limit", &codex, false)),
        LandMode::Direct
    );
    let r = push(&w, &b, "Checkout button copy", &claude, false);
    assert!(
        matches!(mode(&r), LandMode::Together { .. }),
        "{:?}",
        mode(&r)
    );

    // the pusher's main and workspace moved to the hub's
    assert_eq!(b.main().unwrap(), w.hub().main().unwrap());
    assert_eq!(r.synced, vec![DEFAULT_WORKSPACE.to_string()]);
    let text = fs::read_to_string(b_dir.join("src/main.0")).unwrap();
    assert!(text.contains("key % 64") && text.contains("return 2"));
    assert!(b.refs().unwrap().drafts.is_empty());
    // the hub kept who did it, and for whom
    let hist = w.hub().history(&w.hub().main().unwrap(), None).unwrap();
    let c = w.hub().change(hist[0].1.change.as_ref().unwrap()).unwrap();
    assert_eq!(c.actor, claude);
}

#[test]
fn dependent_work_is_ordered_after_on_the_hub() {
    let w = World::new();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let cursor = Actor::agent("cursor", Some("dev"));
    let (a, a_dir) = w.clone("codex", &codex);
    let (b, b_dir) = w.clone("cursor", &cursor);
    edit(
        &a_dir,
        "return base * attempt",
        "return base * attempt * attempt",
    );
    edit(
        &b_dir,
        "return retryDelay(times)",
        "return retryDelay(times) + 1",
    );
    push(&w, &a, "Back off retries", &codex, false);
    let r = push(&w, &b, "Use retryDelay in fetchAll", &cursor, false);
    assert!(matches!(mode(&r), LandMode::Ordered { .. }));
}

#[test]
fn a_conflict_is_saved_on_the_hub_and_the_draft_kept() {
    let w = World::new();
    let dev = Actor::person("dev");
    let cursor = Actor::agent("cursor", Some("harshit"));
    let (a, a_dir) = w.clone("dev", &dev);
    let (b, b_dir) = w.clone("cursor", &cursor);
    edit(&a_dir, "return base * attempt", "return base * attempt + 5");
    edit(&b_dir, "return base * attempt", "return base * attempt * 2");
    push(&w, &a, "Cap retries", &dev, false);
    let before = w.hub().main().unwrap();
    let r = push(&w, &b, "Double the backoff", &cursor, false);
    match r.proposal.unwrap() {
        Proposal::Conflict { decls, .. } => assert_eq!(decls, vec!["retryDelay".to_string()]),
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(w.hub().main().unwrap(), before);
    assert_eq!(w.hub().refs().unwrap().conflicts.len(), 1);
    assert_eq!(b.refs().unwrap().drafts.len(), 1, "the work is never lost");
}

#[test]
fn a_change_sent_for_review_waits_then_lands() {
    let w = World::new();
    let gemini = Actor::agent("gemini", Some("meera"));
    let (a, a_dir) = w.clone("gemini", &gemini);
    edit(&a_dir, "return key", "return key + 1");
    let r = push(&w, &a, "Off by one", &gemini, true);
    assert!(matches!(r.proposal, Some(Proposal::Open { .. })));
    let open = w.hub().remote_refs().unwrap().open;
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].actor, gemini);
    let main = w.hub().main().unwrap();
    let landed = w.hub().land_open(&open[0].id, &w.harshit).unwrap();
    assert!(matches!(landed, Proposal::Landed { .. }));
    assert_ne!(w.hub().main().unwrap(), main);
    assert!(w.hub().remote_refs().unwrap().open.is_empty());
}

#[test]
fn pull_brings_a_clone_up_to_the_hub_and_keeps_its_edits() {
    let w = World::new();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let meera = Actor::person("meera");
    let (a, a_dir) = w.clone("codex", &codex);
    let (b, b_dir) = w.clone("meera", &meera);
    edit(&a_dir, "return key", "return key % 64");
    push(&w, &a, "Per-key rate limit", &codex, false);
    edit(&b_dir, "return 1", "return 7");
    b.pull(DEFAULT_WORKSPACE, "origin", &folder(&w.hub), &meera)
        .unwrap();
    assert_eq!(b.main().unwrap(), w.hub().main().unwrap());
    let text = fs::read_to_string(b_dir.join("src/main.0")).unwrap();
    assert!(text.contains("key % 64"), "pulled the landing");
    assert!(text.contains("return 7"), "kept the unrecorded edit");
    assert_eq!(b.refs().unwrap().remotes["origin"], w.hub().main().unwrap());
}

#[test]
fn landings_made_locally_push_as_a_fast_forward() {
    let w = World::new();
    let meera = Actor::person("meera");
    let (a, a_dir) = w.clone("meera", &meera);
    edit(&a_dir, "return 1", "return 9");
    a.record(DEFAULT_WORKSPACE, "Local landing", &meera)
        .unwrap();
    a.land(DEFAULT_WORKSPACE, None, &meera).unwrap();
    let r = a
        .push_to(
            DEFAULT_WORKSPACE,
            "origin",
            &folder(&w.hub),
            None,
            false,
            &meera,
        )
        .unwrap();
    assert_eq!(r.landings, 1);
    assert_eq!(w.hub().main().unwrap(), a.main().unwrap());
}

#[test]
fn diverged_mains_are_refused_not_overwritten() {
    let w = World::new();
    let (a, a_dir) = w.clone("a", &Actor::person("a"));
    let (b, b_dir) = w.clone("b", &Actor::person("b"));
    edit(&a_dir, "return 1", "return 9");
    a.record(DEFAULT_WORKSPACE, "A", &Actor::person("a"))
        .unwrap();
    a.land(DEFAULT_WORKSPACE, None, &Actor::person("a"))
        .unwrap();
    a.push_to(
        DEFAULT_WORKSPACE,
        "origin",
        &folder(&w.hub),
        None,
        false,
        &Actor::person("a"),
    )
    .unwrap();
    edit(&b_dir, "return key", "return 0");
    b.record(DEFAULT_WORKSPACE, "B", &Actor::person("b"))
        .unwrap();
    b.land(DEFAULT_WORKSPACE, None, &Actor::person("b"))
        .unwrap();
    let hub_main = w.hub().main().unwrap();
    // b hasn't seen a's landing, so the hub's main is unknown to it: the
    // history push is refused by the hub, which won't move main backwards
    let res = b.push_to(
        DEFAULT_WORKSPACE,
        "origin",
        &folder(&w.hub),
        None,
        false,
        &Actor::person("b"),
    );
    assert!(res.is_err() || w.hub().main().unwrap() == hub_main);
    assert_eq!(w.hub().main().unwrap(), hub_main);
    // and once b has fetched, it is told why
    b.fetch_from("origin", &folder(&w.hub)).unwrap();
    let err = b
        .push_to(
            DEFAULT_WORKSPACE,
            "origin",
            &folder(&w.hub),
            None,
            false,
            &Actor::person("b"),
        )
        .unwrap_err();
    assert!(err.to_string().contains("both moved"), "{err}");
}

#[test]
fn a_forged_object_is_refused() {
    let w = World::new();
    let hub = w.hub();
    let forged = Object {
        domain: "decl".into(),
        hash: hash::decl_hash("fn a() {}\n"),
        data: "fn evil() {}\n".into(),
    };
    assert!(hub.receive(&[forged]).is_err());
}

#[test]
fn an_empty_hub_cannot_be_cloned() {
    let tmp = tempfile::tempdir().unwrap();
    let hub = tmp.path().join("hub");
    Repo::init_bare(&hub, &Actor::person("x")).unwrap();
    let err = Repo::clone_from(
        "hub",
        &folder(&hub),
        &tmp.path().join("c"),
        &Actor::person("x"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("empty"));
}

#[test]
fn a_change_from_an_unknown_base_is_refused() {
    let w = World::new();
    // a repository the hub has never seen
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("a.md"), "hello\n").unwrap();
    let other = Repo::init(tmp.path(), &Actor::person("x"), vec![]).unwrap();
    fs::write(tmp.path().join("a.md"), "hello there\n").unwrap();
    let c = other
        .record(DEFAULT_WORKSPACE, "edit", &Actor::person("x"))
        .unwrap();
    let err = w.hub().propose(&c, true, &Actor::person("x")).unwrap_err();
    assert!(err.to_string().contains("pull first"), "{err}");
}

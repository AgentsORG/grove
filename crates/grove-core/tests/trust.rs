//! Identity, review rules, approvals, remembered resolutions, notes, presence
//! and pull requests.

use grove_core::trust::{Review, SigStatus};
use grove_core::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SRC: &str = "pub fn parse(b: u32) -> u32 {
    return b + 1
}

fn retryDelay(attempt: u32) -> u32 {
    return 100 * attempt
}

fn fetchAll(times: u32) -> u32 {
    return retryDelay(times) + parse(times)
}
";

struct T {
    _tmp: tempfile::TempDir,
    _home: tempfile::TempDir,
    repo: Repo,
    root: PathBuf,
}

fn setup() -> T {
    let tmp = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    // keys live under GROVE_HOME; each test gets its own
    std::env::set_var("GROVE_HOME", home.path());
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.0"), SRC).unwrap();
    let repo = Repo::init(&root, &Actor::person("harshit"), vec![]).unwrap();
    T {
        _tmp: tmp,
        _home: home,
        repo,
        root,
    }
}

impl T {
    fn ws(&self, name: &str, who: &Actor) -> PathBuf {
        self.repo
            .add_workspace(name, &self.root.parent().unwrap().join(name), who)
            .unwrap()
    }
}

fn edit(dir: &Path, from: &str, to: &str) {
    let p = dir.join("main.0");
    let t = fs::read_to_string(&p).unwrap();
    assert!(t.contains(from), "{from}");
    fs::write(&p, t.replacen(from, to, 1)).unwrap();
}

fn rejected_for(o: LandOutcome) -> Vec<Evidence> {
    match o {
        LandOutcome::Rejected { evidence, .. } => evidence,
        o => panic!("expected a refusal, got {o:?}"),
    }
}

fn landed(o: LandOutcome) -> Vec<Evidence> {
    match o {
        LandOutcome::Landed { evidence, .. } => evidence,
        o => panic!("did not land: {o:?}"),
    }
}

// One test so GROVE_HOME (process-wide) is never shared between threads.
#[test]
fn trust_review_and_working_together() {
    signed_changes_and_tampering();
    required_signatures();
    review_rules_need_a_person();
    resolutions_are_remembered();
    notes_and_presence();
    pull_request_branch();
    approvals_must_be_signed_when_signatures_are_required();
}

fn signed_changes_and_tampering() {
    let t = setup();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let key = trust::new_key(&codex).unwrap();
    let a = t.ws("codex", &codex);
    edit(&a, "100 * attempt", "200 * attempt");
    let mut c = t.repo.record("codex", "Double", &codex).unwrap();
    assert!(matches!(
        trust::verify(&c, &Default::default()),
        SigStatus::Valid { trusted: false, .. }
    ));
    let mut keys = std::collections::BTreeMap::new();
    keys.insert("agent:codex".to_string(), key);
    assert!(matches!(
        trust::verify(&c, &keys),
        SigStatus::Valid { trusted: true, .. }
    ));
    c.intent = "Something else".into();
    assert_eq!(trust::verify(&c, &keys), SigStatus::Invalid);
}

fn required_signatures() {
    let t = setup();
    t.repo
        .update_config(|c| {
            c.review.require_signed = true;
            Ok(())
        })
        .unwrap();
    let meera = Actor::person("meera");
    let a = t.ws("meera", &meera);
    edit(&a, "100 * attempt", "300 * attempt");
    t.repo.record("meera", "Triple", &meera).unwrap();
    let ev = rejected_for(t.repo.land("meera", None, &meera).unwrap());
    assert!(ev.iter().any(|e| e.check == "signature" && !e.ok));

    // a key, trusted, and the next version lands
    let key = trust::new_key(&meera).unwrap();
    t.repo
        .update_config(|c| {
            c.keys.insert("person:meera".into(), key.clone());
            Ok(())
        })
        .unwrap();
    edit(&a, "300 * attempt", "301 * attempt");
    t.repo.record("meera", "Triple-ish", &meera).unwrap();
    let ev = landed(t.repo.land("meera", None, &meera).unwrap());
    assert!(ev
        .iter()
        .any(|e| e.check == "signature" && e.ok && e.detail.starts_with("trusted")));
}

fn review_rules_need_a_person() {
    let t = setup();
    t.repo
        .update_config(|c| {
            c.review = Review {
                signature: true,
                capabilities: true,
                hot_links: Some(2),
                protected: vec!["secrets".into()],
                ..Review::default()
            };
            Ok(())
        })
        .unwrap();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let a = t.ws("codex", &codex);

    // a public signature change waits for a person
    edit(
        &a,
        "pub fn parse(b: u32) -> u32 {",
        "pub fn parse(b: u32, strict: Bool) -> u32 {",
    );
    let v1 = t.repo.record("codex", "Strict parsing", &codex).unwrap();
    let ev = rejected_for(t.repo.land("codex", None, &codex).unwrap());
    assert!(
        ev.iter().any(|e| e.check == "review: needs a person"
            && e.detail.contains("public signature of parse")),
        "{ev:?}"
    );
    assert!(
        t.repo
            .approve(&v1.id, &Actor::agent("cursor", None))
            .is_err(),
        "agents don't approve"
    );
    t.repo.approve(&v1.id, &Actor::person("rishitabh")).unwrap();
    // a new version needs a new yes
    edit(&a, "return b + 1", "return b + 2");
    let v2 = t.repo.record("codex", "Strict parsing", &codex).unwrap();
    assert_eq!(v2.version, 2);
    rejected_for(t.repo.land("codex", None, &codex).unwrap());
    t.repo.approve(&v2.id, &Actor::person("harshit")).unwrap();
    let ev = landed(t.repo.land("codex", None, &codex).unwrap());
    assert!(ev
        .iter()
        .any(|e| e.check == "review" && e.detail.contains("approved by harshit")));

    // new network access
    edit(
        &a,
        "return 100 * attempt",
        "let x: u32 = std.net.ping()\n    return 100 * attempt",
    );
    t.repo.record("codex", "Ping", &codex).unwrap();
    let ev = rejected_for(t.repo.land("codex", None, &codex).unwrap());
    assert!(
        ev.iter()
            .any(|e| e.detail.contains("retryDelay now uses std.net")),
        "{ev:?}"
    );

    // a protected path
    let meera = Actor::person("meera");
    let b = t.ws("meera", &meera);
    fs::create_dir_all(b.join("secrets")).unwrap();
    fs::write(b.join("secrets/keys.txt"), "x").unwrap();
    t.repo.record("meera", "Keys", &meera).unwrap();
    let ev = rejected_for(t.repo.land("meera", None, &meera).unwrap());
    assert!(
        ev.iter()
            .any(|e| e.detail.contains("secrets/keys.txt is protected")),
        "{ev:?}"
    );
}

fn resolutions_are_remembered() {
    let t = setup();
    let (dev, cur) = (
        Actor::person("dev"),
        Actor::agent("cursor", Some("harshit")),
    );
    let clash = |a: &str, b: &str| {
        let pa = t.ws(a, &dev);
        let pb = t.ws(b, &cur);
        edit(
            &pa,
            "return 100 * attempt",
            "return min(100 * attempt, 5000)",
        );
        edit(&pb, "return 100 * attempt", "return 100 * pow2(attempt)");
        t.repo.record(a, "Cap", &dev).unwrap();
        t.repo.record(b, "Exponential", &cur).unwrap();
        landed(t.repo.land(a, None, &dev).unwrap());
        t.repo.land(b, None, &cur).unwrap()
    };
    let id = match clash("a", "b") {
        LandOutcome::Conflict { id, .. } => id,
        o => panic!("{o:?}"),
    };
    // someone combines both intents and lands it, then closes the conflict
    let me = Actor::person("harshit");
    t.repo.sync(DEFAULT_WORKSPACE, &me).unwrap();
    edit(
        &t.root,
        "return min(100 * attempt, 5000)",
        "return min(100 * pow2(attempt), 5000)",
    );
    t.repo.record(DEFAULT_WORKSPACE, "Combine", &me).unwrap();
    landed(t.repo.land(DEFAULT_WORKSPACE, None, &me).unwrap());
    t.repo.close_conflict(&id, &me).unwrap();

    // rewind everything (resolutions are kept), and the same clash happens again
    while t.repo.oplog().unwrap().iter().any(|e| {
        e.kind != "init"
            && e.kind != "undo"
            && !t
                .repo
                .oplog()
                .unwrap()
                .iter()
                .any(|u| u.undoes == Some(e.seq))
    }) {
        t.repo.undo(&me).unwrap();
    }
    let ev = landed(clash("a2", "b2"));
    assert!(
        ev.iter().any(|e| e.check == "grove: reused a resolution"),
        "{ev:?}"
    );
    assert!(
        t.repo.main_view().unwrap().files()["main.0"].contains("min(100 * pow2(attempt), 5000)")
    );
}

fn notes_and_presence() {
    let t = setup();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let a = t.ws("codex", &codex);
    edit(&a, "100 * attempt", "150 * attempt");
    let c = t.repo.record("codex", "Slower", &codex).unwrap();
    t.repo
        .note(&c.id, "measured on staging", &Actor::person("meera"))
        .unwrap();
    assert_eq!(t.repo.notes(&c.id).unwrap()[0].text, "measured on staging");
    let who = t.repo.who().unwrap();
    let mine = who.iter().find(|p| p.workspace == "codex").unwrap();
    assert_eq!(
        mine.draft.as_ref().unwrap().1,
        vec!["retryDelay".to_string()]
    );
}

fn pull_request_branch() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let t = setup();
    let hub = t.root.parent().unwrap().join("hub.git");
    Command::new("git")
        .args([
            "init",
            "--bare",
            "--quiet",
            "--initial-branch=main",
            hub.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let a = t.ws("codex", &codex);
    edit(&a, "100 * attempt", "120 * attempt");
    let c = t.repo.record("codex", "Gentler", &codex).unwrap();
    let pr = git::pr(&t.repo, hub.to_str().unwrap(), &c).unwrap();
    assert_eq!(pr.url, None, "not a GitHub remote, so no gh");
    let show = Command::new("git")
        .args(["show", &format!("{}:main.0", pr.branch)])
        .current_dir(&hub)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&show.stdout).contains("120 * attempt"));
    assert_eq!(
        git::github_repo("git@github.com:AgentsORG/grove.git").as_deref(),
        Some("AgentsORG/grove")
    );
    assert_eq!(
        git::github_repo("https://github.com/AgentsORG/grove").as_deref(),
        Some("AgentsORG/grove")
    );
}

fn approvals_must_be_signed_when_signatures_are_required() {
    let t = setup();
    let codex = Actor::agent("codex", Some("rishitabh"));
    let rishitabh = Actor::person("rishitabh");
    let (ck, rk) = (
        trust::new_key(&codex).unwrap(),
        trust::new_key(&rishitabh).unwrap(),
    );
    t.repo
        .update_config(|c| {
            c.review.signature = true;
            c.review.require_signed = true;
            c.keys.insert("agent:codex".into(), ck.clone());
            Ok(())
        })
        .unwrap();
    let a = t.ws("codex", &codex);
    edit(
        &a,
        "pub fn parse(b: u32) -> u32 {",
        "pub fn parse(b: u32, strict: Bool) -> u32 {",
    );
    let c = t.repo.record("codex", "Strict", &codex).unwrap();
    // someone claiming to be a person, with no key: doesn't count
    t.repo.approve(&c.id, &Actor::person("mallory")).unwrap();
    rejected_for(t.repo.land("codex", None, &codex).unwrap());
    // a signed approval from a key the repo doesn't trust yet: doesn't count
    t.repo.approve(&c.id, &rishitabh).unwrap();
    rejected_for(t.repo.land("codex", None, &codex).unwrap());
    // trusted: lands
    t.repo
        .update_config(|c| {
            c.keys.insert("person:rishitabh".into(), rk.clone());
            Ok(())
        })
        .unwrap();
    let ev = landed(t.repo.land("codex", None, &codex).unwrap());
    assert!(
        ev.iter().any(|e| e.check == "review"
            && e.detail.contains("approved by rishitabh")
            && !e.detail.contains("mallory")),
        "{ev:?}"
    );
}

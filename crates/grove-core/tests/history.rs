//! Git's history tools in their Grove form, end to end.

use grove_core::*;
use std::fs;
use std::path::{Path, PathBuf};

const SRC: &str = "fn retryDelay(attempt: u32) -> u32 {
    let base: u32 = 100
    let cap: u32 = 5000
    return base * attempt
}

fn fetchAll(times: u32) -> u32 {
    return retryDelay(times)
}

fn parse(b: u32) -> u32 {
    return b + 1
}
";

struct T {
    _tmp: tempfile::TempDir,
    repo: Repo,
    root: PathBuf,
    me: Actor,
}

fn setup() -> T {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.0"), SRC).unwrap();
    let me = Actor::person("meera");
    let repo = Repo::init(&root, &me, vec![]).unwrap();
    T {
        _tmp: tmp,
        repo,
        root,
        me,
    }
}

fn edit(dir: &Path, from: &str, to: &str) {
    let p = dir.join("main.0");
    let t = fs::read_to_string(&p).unwrap();
    assert!(t.contains(from), "{from}");
    fs::write(&p, t.replacen(from, to, 1)).unwrap();
}

fn landed(o: LandOutcome) -> LandMode {
    match o {
        LandOutcome::Landed { mode, .. } => mode,
        o => panic!("did not land: {o:?}"),
    }
}

fn main_src(t: &T) -> String {
    t.repo.main_view().unwrap().files()["main.0"].clone()
}

impl T {
    fn ws(&self, name: &str) -> PathBuf {
        self.repo
            .add_workspace(name, &self.root.parent().unwrap().join(name), &self.me)
            .unwrap()
    }
    fn change(&self, ws: &str, from: &str, to: &str, intent: &str) -> LandOutcome {
        let dir = if ws == DEFAULT_WORKSPACE {
            self.root.clone()
        } else {
            self.root.parent().unwrap().join(ws)
        };
        edit(&dir, from, to);
        self.repo.record(ws, intent, &self.me).unwrap();
        self.repo.land(ws, None, &self.me).unwrap()
    }
}

#[test]
fn two_edits_to_different_lines_of_one_function_merge() {
    let t = setup();
    let a = t.ws("a");
    let b = t.ws("b");
    edit(&a, "= 100", "= 250");
    edit(&b, "base * attempt", "base * pow2(attempt)");
    t.repo.record("a", "Raise base", &t.me).unwrap();
    t.repo.record("b", "Exponential", &t.me).unwrap();
    landed(t.repo.land("a", None, &t.me).unwrap());
    match t.repo.land("b", None, &t.me).unwrap() {
        LandOutcome::Landed { mode, evidence, .. } => {
            assert!(matches!(mode, LandMode::Ordered { .. }));
            assert!(evidence
                .iter()
                .any(|e| e.check == "grove: merged inside" && e.detail == "retryDelay"));
        }
        o => panic!("{o:?}"),
    }
    let s = main_src(&t);
    assert!(s.contains("= 250") && s.contains("pow2(attempt)"), "{s}");
}

#[test]
fn revert_undoes_a_change_even_a_rename() {
    let t = setup();
    landed(t.change(DEFAULT_WORKSPACE, "return b + 1", "return b + 2", "Two"));
    let text = fs::read_to_string(t.root.join("main.0")).unwrap();
    fs::write(
        t.root.join("main.0"),
        text.replace("retryDelay(", "delayFor("),
    )
    .unwrap();
    t.repo
        .record(DEFAULT_WORKSPACE, "Rename retryDelay", &t.me)
        .unwrap();
    let rename = match t.repo.land(DEFAULT_WORKSPACE, None, &t.me).unwrap() {
        LandOutcome::Landed { change, .. } => change,
        o => panic!("{o:?}"),
    };
    assert!(main_src(&t).contains("delayFor(times)"));
    landed(t.repo.revert(DEFAULT_WORKSPACE, &rename.id, &t.me).unwrap());
    let s = main_src(&t);
    assert!(
        s.contains("fn retryDelay(")
            && s.contains("return retryDelay(times)")
            && s.contains("b + 2"),
        "{s}"
    );
}

#[test]
fn pick_brings_a_change_from_a_lane() {
    let t = setup();
    t.repo.lane_new("experiment", None, &t.me).unwrap();
    t.ws("x");
    t.repo.lane_use("x", Some("experiment"), &t.me).unwrap();
    let c = match t.change("x", "return b + 1", "return b * 3", "Triple") {
        LandOutcome::Landed { change, .. } => change,
        o => panic!("{o:?}"),
    };
    assert!(
        !main_src(&t).contains("b * 3"),
        "the lane doesn't touch main"
    );
    landed(t.repo.pick(DEFAULT_WORKSPACE, &c.id, &t.me).unwrap());
    assert!(main_src(&t).contains("b * 3"));
}

#[test]
fn a_lane_lands_on_main_as_one_change() {
    let t = setup();
    t.repo.lane_new("checkout-v2", None, &t.me).unwrap();
    t.ws("lane");
    t.repo.lane_use("lane", Some("checkout-v2"), &t.me).unwrap();
    landed(t.change("lane", "return b + 1", "return b + 5", "Five"));
    landed(t.change("lane", "let cap: u32 = 5000", "let cap: u32 = 9000", "Cap"));
    // main moves meanwhile, elsewhere
    landed(t.change(
        DEFAULT_WORKSPACE,
        "return retryDelay(times)",
        "return retryDelay(times) + 1",
        "Plus one",
    ));
    match t.repo.lane_land("checkout-v2", &t.me).unwrap() {
        LandOutcome::Landed { change, .. } => {
            assert_eq!(change.intent, "Land lane checkout-v2 (2 changes)")
        }
        o => panic!("{o:?}"),
    }
    let s = main_src(&t);
    assert!(
        s.contains("b + 5") && s.contains("9000") && s.contains("(times) + 1"),
        "{s}"
    );
    t.repo.lane_use("lane", None, &t.me).unwrap();
    t.repo.lane_drop("checkout-v2", &t.me).unwrap();
}

#[test]
fn restore_shelve_and_unshelve() {
    let t = setup();
    edit(&t.root, "return b + 1", "return b + 7");
    assert_eq!(
        t.repo
            .restore(DEFAULT_WORKSPACE, &["main.0".into()])
            .unwrap(),
        vec!["main.0"]
    );
    assert!(t.repo.status(DEFAULT_WORKSPACE).unwrap().is_empty());

    edit(&t.root, "return b + 1", "return b + 8");
    fs::write(t.root.join("new.md"), "# new\n").unwrap();
    assert_eq!(t.repo.shelve(DEFAULT_WORKSPACE, "wip", &t.me).unwrap(), 2);
    assert!(t.repo.status(DEFAULT_WORKSPACE).unwrap().is_empty());
    assert!(!t.root.join("new.md").exists());
    // main moves, then the shelf comes back on top of it
    landed(t.change(DEFAULT_WORKSPACE, "= 100", "= 150", "Base"));
    t.repo.unshelve(DEFAULT_WORKSPACE, "wip", &t.me).unwrap();
    let s = fs::read_to_string(t.root.join("main.0")).unwrap();
    assert!(s.contains("b + 8") && s.contains("= 150"), "{s}");
    assert!(t.root.join("new.md").exists());
}

#[test]
fn tags_find_evolve_shortlog_and_text_diff() {
    let t = setup();
    landed(t.change(DEFAULT_WORKSPACE, "return b + 1", "return b + 2", "Two"));
    let tagged = t.repo.tag("v0.1.0", None, &t.me).unwrap();
    assert_eq!(t.repo.resolve_landing("v0.1.0").unwrap(), tagged);
    assert!(t.repo.tag("v0.1.0", None, &t.me).is_err());

    let hits = t.repo.find("retryDelay(times)").unwrap();
    assert_eq!(
        hits,
        vec![(
            "main.0".to_string(),
            8,
            "    return retryDelay(times)".to_string()
        )]
    );

    edit(&t.root, "= 100", "= 101");
    let v1 = t
        .repo
        .record(DEFAULT_WORKSPACE, "first try", &t.me)
        .unwrap();
    edit(&t.root, "= 101", "= 102");
    t.repo
        .record(DEFAULT_WORKSPACE, "second try", &t.me)
        .unwrap();
    let versions = t.repo.evolve(&v1.id).unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|c| c.intent.as_str())
            .collect::<Vec<_>>(),
        vec!["second try", "first try"]
    );

    let d = t.repo.diff_text(DEFAULT_WORKSPACE, None).unwrap();
    assert!(
        d.contains("--- a/main.0")
            && d.contains("-    let base: u32 = 100")
            && d.contains("+    let base: u32 = 102"),
        "{d}"
    );

    assert_eq!(t.repo.shortlog().unwrap(), vec![("meera".to_string(), 1)]);
}

#[test]
fn bisect_finds_the_change_that_broke_it() {
    let t = setup();
    landed(t.change(DEFAULT_WORKSPACE, "return b + 1", "return b + 2", "Fine"));
    landed(t.change(
        DEFAULT_WORKSPACE,
        "let cap: u32 = 5000",
        "let cap: u32 = 0",
        "Oops",
    ));
    landed(t.change(
        DEFAULT_WORKSPACE,
        "return b + 2",
        "return b + 3",
        "Also fine",
    ));
    // the "test": fail when cap is 0
    let cmd = if cfg!(windows) {
        "findstr /C:\"cap: u32 = 0\" main.0 >nul && exit 1 || exit 0"
    } else {
        "! grep -q 'cap: u32 = 0' main.0"
    };
    let b = t.repo.bisect(cmd, None).unwrap();
    assert_eq!(b.change.unwrap().intent, "Oops");
}

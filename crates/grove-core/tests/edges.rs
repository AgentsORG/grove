//! Exact edges from Zero's resolver, and the fallback to names.

use grove_core::*;
use std::collections::BTreeSet;
use std::fs;

const MAIN: &str = "pub fn main(world: World) -> Void raises {
    let x: i32 = scale(add(1, 2))
    check world.out.write(\"hi\\n\")
}

fn add(l: i32, r: i32) -> i32 {
    return mul(l, r) + limit
}

fn mul(l: i32, r: i32) -> i32 {
    return l * r
}

fn scale(mul: i32) -> i32 {
    return mul * 2
}

const limit: i32 = 64
";

const TOML: &str = "[package]
name = \"edges\"
version = \"0.1.0\"
license = \"MIT\"

[targets.cli]
kind = \"exe\"
main = \"src/main.0\"
defaultTarget = \"linux-musl-x64\"
devTarget = \"host\"
releaseProfile = \"release-small\"

[deps]

[profiles.dev]
inherits = \"dev\"

[profiles.release-small]
inherits = \"release-small\"
";

fn named(view: &View) -> BTreeSet<(String, String)> {
    let name = |id: &DeclId| view.decls[id].entry.name.clone();
    view.edges()
        .iter()
        .filter(|e| e.kind == EdgeKind::Code)
        .map(|e| (name(&e.from), name(&e.to)))
        .collect()
}

#[test]
fn zero_resolver_edges_and_name_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("GROVE_HOME", home.path());
    let root = tmp.path().join("r");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.0"), MAIN).unwrap();
    fs::write(root.join("zero.toml"), TOML).unwrap();
    let repo = Repo::init(&root, &Actor::person("harshit"), vec![]).unwrap();
    let view = repo.main_view().unwrap();

    // by name: the parameter `mul` inside scale looks like a call to mul
    std::env::set_var("GROVE_EDGES", "names");
    let by_name = named(&view);
    std::env::remove_var("GROVE_EDGES");
    assert!(by_name.contains(&("scale".into(), "mul".into())));

    if zero::zero_bin().is_none() {
        eprintln!("zero isn't installed; skipping the resolver half");
        return;
    }
    let exact = named(&view);
    let want: BTreeSet<(String, String)> = [
        ("main", "scale"),
        ("main", "add"),
        ("add", "mul"),
        ("add", "limit"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(exact, want, "Zero knows scale's `mul` is a parameter");

    // the answer is cached by snapshot, so the next ask doesn't run Zero
    let cache = home.path().join("cache").join("edges");
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 1);
    assert_eq!(named(&view), want);

    // code that doesn't compile falls back to names, and that's cached too
    let mut broken = view.clone();
    let mul = broken
        .decls
        .values_mut()
        .find(|n| n.entry.name == "mul")
        .unwrap();
    mul.source = "fn mul(l: i32, r: i32) -> i32 {\n    return l * \n}\n".into();
    assert!(named(&broken).contains(&("scale".into(), "mul".into())));
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 2);
}

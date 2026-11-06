//! Partial workspaces (git's sparse checkout) and signed tags.

use grove_core::*;
use std::fs;

#[test]
fn partial_workspaces_and_signed_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("GROVE_HOME", home.path());
    let root = tmp.path().join("r");
    fs::create_dir_all(root.join("billing")).unwrap();
    fs::create_dir_all(root.join("http")).unwrap();
    fs::write(
        root.join("billing/price.0"),
        "fn price() -> u32 {\n    return 10\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("http/route.0"),
        "fn route() -> u32 {\n    return 1\n}\n",
    )
    .unwrap();
    fs::write(root.join("README.md"), "# shop\n").unwrap();
    let me = Actor::person("harshit");
    let repo = Repo::init(&root, &me, vec![]).unwrap();

    // a workspace that holds only billing
    let codex = Actor::agent("codex", Some("harshit"));
    let ws = repo
        .add_workspace_only(
            "billing",
            &tmp.path().join("billing-ws"),
            &codex,
            &["billing".into()],
        )
        .unwrap();
    assert!(ws.join("billing/price.0").exists());
    assert!(!ws.join("http/route.0").exists() && !ws.join("README.md").exists());
    assert!(
        repo.status("billing").unwrap().is_empty(),
        "files it doesn't hold are not deletions"
    );

    // it edits what it holds and lands; the rest of main is untouched
    fs::write(
        ws.join("billing/price.0"),
        "fn price() -> u32 {\n    return 12\n}\n",
    )
    .unwrap();
    repo.record("billing", "Raise price", &codex).unwrap();
    assert!(matches!(
        repo.land("billing", None, &codex).unwrap(),
        LandOutcome::Landed { .. }
    ));
    let files = repo.main_view().unwrap().files();
    assert!(files["billing/price.0"].contains("return 12"));
    assert_eq!(
        files["http/route.0"],
        "fn route() -> u32 {\n    return 1\n}\n"
    );
    assert_eq!(files["README.md"], "# shop\n");

    // someone else changes http; syncing the partial workspace doesn't write it
    fs::write(
        root.join("http/route.0"),
        "fn route() -> u32 {\n    return 2\n}\n",
    )
    .unwrap();
    repo.record(DEFAULT_WORKSPACE, "Route two", &me).unwrap();
    repo.land(DEFAULT_WORKSPACE, None, &me).unwrap();
    repo.sync("billing", &codex).unwrap();
    assert!(!ws.join("http/route.0").exists());
    assert!(repo.status("billing").unwrap().is_empty());

    // signed tags
    let key = trust::new_key(&me).unwrap();
    repo.update_config(|c| {
        c.keys.insert("person:harshit".into(), key.clone());
        Ok(())
    })
    .unwrap();
    let at = repo.tag("v1.0.0", None, &me).unwrap();
    let refs = repo.refs().unwrap();
    let sig = refs.tag_signatures.get("v1.0.0").expect("signed");
    assert_eq!(
        trust::verify_tag("v1.0.0", &at, sig, &repo.config().unwrap().keys),
        Ok("person:harshit".to_string())
    );
    // a signature moved to another tag doesn't verify
    assert!(trust::verify_tag("v9.9.9", &at, sig, &repo.config().unwrap().keys).is_err());
    // someone without a key tags unsigned
    repo.tag("v1.0.1", None, &Actor::person("meera")).unwrap();
    assert!(!repo.refs().unwrap().tag_signatures.contains_key("v1.0.1"));
}

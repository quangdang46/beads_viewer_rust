//! Discovery chain + GitLoader tests.
use std::path::Path;
use std::sync::Mutex;

// Env vars are process-global; serialize every test that touches them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn discovers_fixture_beads_dir() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/small_chain"
    );
    let dir = bv_core::discovery::get_beads_dir(fixture.as_ref()).expect("discovery works");
    assert!(dir.ends_with(".beads"), "resolved to {}", dir.display());
    let jsonl = bv_core::discovery::find_jsonl_path_with_warnings(&dir, |_| {}).unwrap();
    assert!(jsonl.unwrap().file_name().unwrap() == "issues.jsonl");
}

#[test]
fn beads_db_env_takes_priority() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join(format!("bvr-disc-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join(".beads")).unwrap();
    std::env::set_var("BEADS_DIR", tmp.join(".beads"));
    let dir = bv_core::discovery::get_beads_dir(Path::new("/")).unwrap();
    std::env::remove_var("BEADS_DIR");
    assert_eq!(dir, tmp.join(".beads"));
}

#[test]
fn redirect_follow_resolves_target() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join(format!("bvr-redir-{}", std::process::id()));
    let src = tmp.join("_beads");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(tmp.join(".beads")).unwrap();
    std::fs::write(tmp.join(".beads/redirect"), src.display().to_string()).unwrap();
    // No BEADS_* env interference
    std::env::remove_var("BEADS_DIR");
    std::env::remove_var("BEADS_DB");
    let resolved = bv_core::discovery::get_beads_dir(&tmp).unwrap();
    assert_eq!(resolved, src);
}

#[test]
fn redirect_loop_is_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join(format!("bvr-loop-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join(".beads")).unwrap();
    std::fs::write(
        tmp.join(".beads/redirect"),
        tmp.join(".beads").display().to_string(),
    )
    .unwrap();
    std::env::remove_var("BEADS_DIR");
    std::env::remove_var("BEADS_DB");
    let result = bv_core::discovery::get_beads_dir(&tmp);
    assert!(result.is_err(), "loop must error, not silently fall back");
}

#[test]
fn git_loader_resolves_head_and_loads_selfrepo() {
    let repo = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let loader = bv_core::discovery::GitLoader::new(repo);
    let sha = loader.resolve_revision("HEAD").expect("HEAD resolves");
    assert_eq!(sha.len(), 40);
    let _ = loader.load_at("HEAD").expect("load_at HEAD works");
}

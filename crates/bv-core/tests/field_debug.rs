use bv_core::discovery::load_issues_from_repo;
use std::path::Path;
#[test]
fn debug_fields() {
    let cwd = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let (issues, _) = load_issues_from_repo(Path::new(cwd)).unwrap();
    println!("count={}", issues.len());
    let is = issues
        .iter()
        .find(|i| i.id == "beads_viewer_rust-p3-dispatch-3lv")
        .unwrap();
    println!(
        "status={:?} labels={:?} srcrepo={:?} deps={}",
        is.status,
        is.labels,
        is.source_repo,
        is.dependencies.len()
    );
    if let Some(d) = is.dependencies.first() {
        println!(
            "dep0 on={:?} type={:?} ca={:?} by={:?}",
            d.effective_depends_on(),
            d.r#type,
            d.created_at,
            d.created_by
        );
    }
    panic!("dump");
}

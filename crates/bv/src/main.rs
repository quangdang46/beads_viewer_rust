fn main() {
    // Placeholder binary. Real clap CLI surface lands with bead
    // beads_viewer_rust-p3-cli-surface-r6c (Phase 3a).
    std::process::exit(match std::env::args().nth(1).as_deref() {
        Some("--version") => {
            println!("bvr 0.21.0 (FORT pre-release scaffold)");
            0
        }
        _ => {
            eprintln!("bvr: under construction — robot flags arrive in Phase 3.");
            eprintln!("Use the Go reference build for now: beads_viewer/bv");
            2
        }
    });
}

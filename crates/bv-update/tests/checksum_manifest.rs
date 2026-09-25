//! `parse_checksums` must reproduce Go `parseChecksumData`
//! (`beads_viewer/pkg/updater/updater.go:1056-1097`) exactly.
//!
//! The subtle case is the GNU binary-mode marker. Go keys the map by
//! `strings.TrimSpace(line[len(rawHash):])`, so a manifest line
//! `<hash> *file.tar.gz` produces the key `*file.tar.gz` and a later lookup for
//! `file.tar.gz` **misses**. An implementation that strips the `*` accepts a
//! manifest Go rejects and installs an archive the Go updater would refuse.

use bv_update::update::parse_checksums;

const HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const HASH2: &str = "5f2b3a1c9d8e7f60514233445566778899aabbccddeeff001122334455667788";

/// A platform lookup against a parsed manifest — the operation the `*` bug
/// actually changes.
fn lookup(manifest: &str, filename: &str) -> Option<String> {
    parse_checksums(manifest).ok()?.get(filename).cloned()
}

#[test]
fn gnu_binary_marker_is_part_of_the_key_so_lookup_misses() {
    let manifest = format!("{HASH} *bvr-v0.1.8-linux-x86_64.tar.gz\n");
    let parsed = parse_checksums(&manifest).unwrap();

    // Go's key retains the '*'.
    assert_eq!(
        parsed.get("*bvr-v0.1.8-linux-x86_64.tar.gz"),
        Some(&HASH.to_string()),
        "Go keys the entry as '*<name>'"
    );
    // …and the plain-name lookup therefore misses, which is what makes Go
    // refuse the release. Stripping the '*' here would make Rust install where
    // Go errors — the exact regression this test exists to catch.
    assert_eq!(
        lookup(&manifest, "bvr-v0.1.8-linux-x86_64.tar.gz"),
        None,
        "a GNU binary-mode manifest must not satisfy a plain-name lookup"
    );
}

#[test]
fn plain_and_space_separated_entries_key_by_the_bare_name() {
    // Two spaces (what `shasum -a 256` emits) and one space both key the bare
    // name, because Go trims the remainder of the line.
    for sep in ["  ", " ", "\t"] {
        let manifest = format!("{HASH}{sep}bvr-v0.1.8-linux-x86_64.tar.gz\n");
        assert_eq!(
            lookup(&manifest, "bvr-v0.1.8-linux-x86_64.tar.gz"),
            Some(HASH.to_string()),
            "separator {sep:?} should still key the bare name"
        );
    }
}

#[test]
fn hash_is_lowercased_before_it_becomes_the_value() {
    // Go does `hash := strings.ToLower(rawHash)` but keys the *name* from the
    // original-case `rawHash` length, so an uppercase hash still matches.
    let manifest = format!("{}  archive.tar.gz\n", HASH.to_ascii_uppercase());
    assert_eq!(
        lookup(&manifest, "archive.tar.gz"),
        Some(HASH.to_string()),
        "an uppercase manifest hash must normalise to lowercase"
    );
}

#[test]
fn malformed_lines_are_skipped_not_fatal() {
    // Go `continue`s past every one of these rather than erroring.
    let manifest = format!(
        "{HASH}  good.tar.gz\n\
         \n\
         \x20\x20\x20\n\
         deadbeef  short-hash.tar.gz\n\
         {HASH}zz  not-hex.tar.gz\n\
         {HASH}\n\
         {HASH}  \n\
         onlyhash\n"
    );
    let parsed = parse_checksums(&manifest).unwrap();
    assert_eq!(parsed.len(), 1, "only the well-formed line survives");
    assert_eq!(parsed.get("good.tar.gz"), Some(&HASH.to_string()));
}

#[test]
fn duplicate_entries_are_a_hard_error() {
    // Go: `duplicate checksum entry for %s` (updater.go:1091-1093).
    let manifest = format!("{HASH}  dup.tar.gz\n{HASH2}  dup.tar.gz\n");
    let err = parse_checksums(&manifest).unwrap_err().to_string();
    assert!(
        err.contains("duplicate checksum entry for dup.tar.gz"),
        "got {err:?}"
    );
}

#[test]
fn filenames_may_contain_spaces() {
    // Go's key is the whole trimmed remainder, so spaces inside a filename
    // survive — the reason Go does not key on `parts[1]`.
    let manifest = format!("{HASH}  my release v1.tar.gz\n");
    assert_eq!(
        lookup(&manifest, "my release v1.tar.gz"),
        Some(HASH.to_string())
    );
}

#[test]
fn a_single_line_sha256_sidecar_parses() {
    // The Rust release workflow emits one `<archive>.sha256` per archive
    // (`.github/workflows/release.yml`), not a multi-line checksums.txt, so the
    // one-line shape is the normal one in bvr.
    let manifest = format!("{HASH}  bvr-v0.2.0-macos-aarch64.tar.gz\n");
    let parsed = parse_checksums(&manifest).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed.get("bvr-v0.2.0-macos-aarch64.tar.gz"),
        Some(&HASH.to_string())
    );
}

#[test]
fn a_manifest_over_the_size_cap_is_refused() {
    // Go: `checksum manifest exceeds %d bytes` (updater.go:1057-1058).
    let huge = "a".repeat(bv_update::update::MAX_CHECKSUM_MANIFEST_BYTES + 1);
    assert!(parse_checksums(&huge).is_err());
}

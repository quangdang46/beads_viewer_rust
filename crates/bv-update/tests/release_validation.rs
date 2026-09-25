//! Release identity, asset validation, and checksum-asset selection — the
//! trust boundary that decides whether a release is installable at all.
//!
//! Oracle: `beads_viewer/pkg/updater/updater.go` at commit 18afafa —
//! `validateReleaseIdentity` (748-780), `validateReleaseAsset` (782-817),
//! `assetSHA256Digest` (819-835), `releaseAssetsForUpdate` (837-856),
//! `ValidateReleaseForUpdate` (884-887), `checkedPlatformAsset` (858-878),
//! `FindPlatformAsset` (690-702), `FindChecksumAsset` (722-732),
//! `platformAssetNames` (675-687). Repository identity is deliberately the
//! Rust one; everything else is Go's.

use bv_update::github::{
    checked_platform_asset, release_assets_for_update, validate_release_asset,
    validate_release_for_update, Asset, Release,
};
use bv_update::{asset_name_for, platform_suffix, REPO_NAME, REPO_OWNER};
use std::collections::HashMap;

const DIGEST: &str = "ab";

/// The digest every asset in these fixtures reports, and therefore the value a
/// manifest must carry for `checked_platform_asset` to accept it.
fn digest() -> String {
    DIGEST.repeat(32)
}

fn asset(name: &str) -> Asset {
    Asset {
        name: name.to_string(),
        browser_download_url: format!(
            "https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/download/v1.2.3/{name}"
        ),
        size: 1024,
        digest: format!("sha256:{}", digest()),
        state: "uploaded".to_string(),
    }
}

fn release(tag: &str, assets: Vec<Asset>) -> Release {
    Release {
        tag_name: tag.to_string(),
        html_url: format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/tag/{tag}"),
        draft: false,
        prerelease: false,
        assets,
    }
}

/// Every platform suffix the Rust release workflow publishes, so a test can
/// reason about the whole set of assets on a real release.
fn all_platform_archives(tag: &str) -> Vec<String> {
    [
        "linux-x86_64",
        "linux-aarch64",
        "macos-x86_64",
        "macos-aarch64",
        "windows-x86_64",
    ]
    .iter()
    .map(|p| {
        let ext = if *p == "windows-x86_64" {
            ".zip"
        } else {
            ".tar.gz"
        };
        format!("bvr-{tag}-{p}{ext}")
    })
    .collect()
}

// --- checksum manifest selection -------------------------------------------

/// The real bug: the Rust release workflow publishes one `<archive>.sha256`
/// sidecar per archive and **no** `checksums.txt`. Selecting "the first
/// `.sha256`" hands back another platform's sidecar, whose manifest covers
/// only that other archive — so the install fails on every platform but one.
/// The selection has to be platform-aware.
#[test]
fn sidecar_selection_is_platform_aware() {
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).expect("this platform is published");

    // Matrix order: every foreign platform's sidecar is listed before ours,
    // and each carries a different digest. "First .sha256 wins" therefore
    // picks the wrong one.
    let mut assets = vec![asset(&mine)];
    for archive in all_platform_archives(tag) {
        if archive == mine {
            continue;
        }
        let mut sidecar = asset(&format!("{archive}.sha256"));
        sidecar.digest = format!("sha256:{}", "cd".repeat(32));
        assets.push(sidecar);
    }
    let mut our_sidecar = asset(&format!("{mine}.sha256"));
    our_sidecar.digest = format!("sha256:{}", digest());
    assets.push(our_sidecar);

    let rel = release(tag, assets);
    let chosen = rel.find_checksum_asset().expect("a sidecar exists");
    assert_eq!(
        chosen.name,
        format!("{mine}.sha256"),
        "must pick this platform's sidecar, not the first .sha256 in the list"
    );
    assert_eq!(
        chosen.digest,
        format!("sha256:{}", digest()),
        "the foreign sidecars carry a different digest"
    );
}

#[test]
fn a_checksums_txt_manifest_wins_over_sidecars() {
    // Go's own asset name takes precedence; a real checksums.txt covers every
    // archive, so it is always the better manifest.
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).unwrap();
    let rel = release(
        tag,
        vec![
            asset(&format!("{mine}.sha256")),
            asset("checksums.txt"),
            asset(&mine),
        ],
    );
    assert_eq!(rel.find_checksum_asset().unwrap().name, "checksums.txt");
}

#[test]
fn a_release_with_no_manifest_of_any_kind_is_refused() {
    // Gap: Rust used to install with no verification at all when the release
    // carried no checksum asset. Go refuses (updater.go:849-851).
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).unwrap();
    let rel = release(tag, vec![asset(&mine)]);
    assert_eq!(
        release_assets_for_update(&rel).unwrap_err(),
        "checksums.txt asset is missing"
    );
    assert!(validate_release_for_update(&rel).is_err());
}

#[test]
fn a_foreign_sidecar_manifest_does_not_satisfy_the_platform_lookup() {
    // Belt and braces for the case above: even if the wrong sidecar is used,
    // `checked_platform_asset` refuses instead of installing unverified.
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).unwrap();
    let foreign = all_platform_archives(tag)
        .into_iter()
        .find(|n| n != &mine)
        .expect("another platform is published");
    let rel = release(tag, vec![asset(&mine), asset(&format!("{foreign}.sha256"))]);

    let mut manifest = HashMap::new();
    manifest.insert(foreign.clone(), digest());
    let err = checked_platform_asset(&rel, &manifest).unwrap_err();
    assert!(
        err.contains("has no entry for a"),
        "expected the Go lookup-miss message, got {err:?}"
    );
}

#[test]
fn a_manifest_that_disagrees_with_the_api_is_refused() {
    // Go `checkedPlatformAsset` (updater.go:874-876).
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).unwrap();
    let rel = release(tag, vec![asset(&mine), asset("checksums.txt")]);
    let mut manifest = HashMap::new();
    manifest.insert(mine.clone(), "cd".repeat(32));
    assert_eq!(
        checked_platform_asset(&rel, &manifest).unwrap_err(),
        format!("checksums.txt digest for {mine} disagrees with GitHub release metadata")
    );
}

#[test]
fn a_manifest_agreeing_with_the_api_yields_the_asset_and_digest() {
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).unwrap();
    let rel = release(tag, vec![asset(&mine), asset("checksums.txt")]);
    let mut manifest = HashMap::new();
    manifest.insert(mine.clone(), digest());
    let (found, got) = checked_platform_asset(&rel, &manifest).unwrap();
    assert_eq!(found.name, mine);
    assert_eq!(got, digest());
}

// --- release identity ------------------------------------------------------

#[test]
fn a_draft_prerelease_or_malformed_tag_is_never_installable() {
    let mut draft = release("v1.2.3", vec![]);
    draft.draft = true;
    assert!(validate_release_for_update(&draft)
        .unwrap_err()
        .contains("still a draft"));

    let mut pre = release("v1.2.3", vec![]);
    pre.prerelease = true;
    assert!(validate_release_for_update(&pre)
        .unwrap_err()
        .contains("marked as a prerelease"));

    for (tag, needle) in [
        (" v1.2.3", "surrounding whitespace"),
        ("v1.2.3\n", "surrounding whitespace"),
        ("v1.2", "major.minor.patch"),
        ("v1", "major.minor.patch"),
        ("v1.2.3.4", "one to three numeric components"),
        ("v01.2.3", "leading zero"),
        ("v1.2.3-rc1", "semantic-version prerelease"),
        ("not-a-version", "invalid release tag"),
    ] {
        assert!(
            validate_release_for_update(&release(tag, vec![]))
                .unwrap_err()
                .contains(needle),
            "tag {tag:?} should be refused with {needle:?}"
        );
    }
}

#[test]
fn the_release_page_url_is_pinned_to_this_repo_and_tag() {
    // Go compares against `/<owner>/<repo>/releases/tag/<tag>` (updater.go:775-778).
    let mut forked = release("v1.2.3", vec![]);
    forked.html_url = format!("https://github.com/someone-else/{REPO_NAME}/releases/tag/v1.2.3");
    assert!(validate_release_for_update(&forked)
        .unwrap_err()
        .contains("does not match"));

    for (url, needle) in [
        (
            "http://github.com/a/b/releases/tag/v1.2.3",
            "must use HTTPS on github.com",
        ),
        (
            "https://evil.example.com/a/b/releases/tag/v1.2.3",
            "must use HTTPS on github.com",
        ),
        (
            "https://github.com:443/a/b/releases/tag/v1.2.3",
            "must use HTTPS on github.com",
        ),
        (
            "https://github.com/a/b/releases/tag/v1.2.3?x=1",
            "query, or a fragment",
        ),
        (
            "https://github.com/a/b/releases/tag/v1.2.3#f",
            "query, or a fragment",
        ),
        (
            "https://u:p@github.com/a/b/releases/tag/v1.2.3",
            "credentials",
        ),
    ] {
        let mut r = release("v1.2.3", vec![]);
        r.html_url = url.to_string();
        assert!(
            validate_release_for_update(&r)
                .unwrap_err()
                .contains(needle),
            "{url} should be refused with {needle:?}"
        );
    }
}

// --- asset validation ------------------------------------------------------

#[test]
fn an_asset_needs_uploaded_state_a_bounded_size_and_a_sha256_digest() {
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).unwrap();
    let rel = release(tag, vec![asset(&mine)]);

    let mut not_uploaded = asset(&mine);
    not_uploaded.state = "new".into();
    assert!(
        validate_release_for_update(&release(tag, vec![not_uploaded, asset("checksums.txt")]))
            .unwrap_err()
            .contains("is not uploaded")
    );

    let mut zero_size = asset(&mine);
    zero_size.size = 0;
    assert!(
        validate_release_for_update(&release(tag, vec![zero_size, asset("checksums.txt")]))
            .unwrap_err()
            .contains("invalid size")
    );

    // No digest at all — the field GitHub added later is absent.
    let mut no_digest = asset(&mine);
    no_digest.digest = String::new();
    assert!(
        validate_release_for_update(&release(tag, vec![no_digest, asset("checksums.txt")]))
            .unwrap_err()
            .contains("digest must start with")
    );

    // A path-traversing asset name is refused before it can be joined.
    // `find_platform_asset` only ever matches an exact platform name, so this
    // is checked against `validate_release_asset` directly — which is the
    // function Go applies to whichever asset it is handed.
    let mut traversal = asset("../../etc/passwd");
    traversal.name = "../../etc/passwd".into();
    assert!(
        validate_release_asset(&rel, &traversal, "platform asset", i64::MAX)
            .unwrap_err()
            .contains("unsafe release asset name")
    );

    // Sanity: the unmodified release does validate.
    assert!(validate_release_for_update(&rel).is_err()); // no manifest yet
    assert!(
        validate_release_for_update(&release(tag, vec![asset(&mine), asset("checksums.txt")]))
            .is_ok()
    );
}

#[test]
fn an_asset_download_url_must_point_at_this_release() {
    // Go `validateReleaseAsset` (updater.go:805-812).
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).unwrap();
    let mut off = asset(&mine);
    off.browser_download_url =
        format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/download/v9.9.9/{mine}");
    let err =
        validate_release_for_update(&release(tag, vec![off, asset("checksums.txt")])).unwrap_err();
    assert!(err.contains("URL path"), "got {err:?}");

    let mut foreign_host = asset(&mine);
    foreign_host.browser_download_url = "https://cdn.example.com/malware.tar.gz".into();
    let err =
        validate_release_for_update(&release(tag, vec![foreign_host, asset("checksums.txt")]))
            .unwrap_err();
    assert!(err.contains("must use HTTPS on github.com"), "got {err:?}");
}

#[test]
fn a_full_matrix_release_resolves_to_this_platform() {
    // A real release carries all five archives. The lookup must pick the one
    // for the platform this test is running on, and the matching sidecar —
    // not merely the first entry in the list.
    let tag = "v1.2.3";
    let mine = asset_name_for(tag).expect("this platform is published");
    assert!(all_platform_archives(tag).contains(&mine));

    let mut assets: Vec<Asset> = all_platform_archives(tag)
        .iter()
        .map(|a| {
            let mut asset = asset(a);
            // Only this platform's digest matches, so a wrong pick fails loudly.
            if *a != mine {
                asset.digest = format!("sha256:{}", "cd".repeat(32));
            }
            asset
        })
        .collect();
    // Sidecars, in matrix order, with a foreign one first.
    for archive in all_platform_archives(tag) {
        let mut sidecar = asset(&format!("{archive}.sha256"));
        if archive != mine {
            sidecar.digest = format!("sha256:{}", "ef".repeat(32));
        }
        assets.push(sidecar);
    }

    let rel = release(tag, assets);
    let (found, manifest) = release_assets_for_update(&rel).unwrap();
    assert_eq!(found.name, mine, "platform archive");
    assert_eq!(manifest.name, format!("{mine}.sha256"), "platform sidecar");

    // And the manifest it selected actually covers the archive it selected.
    let mut checksums = HashMap::new();
    checksums.insert(mine.clone(), digest());
    let (checked, got) = checked_platform_asset(&rel, &checksums).unwrap();
    assert_eq!(checked.name, mine);
    assert_eq!(got, digest());
}

#[test]
fn the_platform_asset_name_follows_the_release_workflow_convention() {
    // `bvr-<tag>-<platform><ext>` is what `.github/workflows/release.yml`
    // packages; a drift here silently breaks every install.
    let tag = "v1.2.3";
    let Some(platform) = platform_suffix() else {
        return; // unsupported host: nothing to assert
    };
    let ext = if platform == "windows-x86_64" {
        ".zip"
    } else {
        ".tar.gz"
    };
    assert_eq!(
        asset_name_for(tag).unwrap(),
        format!("bvr-{tag}-{platform}{ext}")
    );
    assert!(all_platform_archives(tag).contains(&asset_name_for(tag).unwrap()));
}

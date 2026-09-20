//! Download → verify → extract → install — port of Go `PerformUpdate`,
//! `Rollback`, `downloadFile`, `verifyChecksum`, `extractBinary`.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::github::{FetchError, Release};
use crate::{BINARY_NAME, REPO_NAME, REPO_OWNER};

/// Max download / extracted-binary size (Go: 512 MiB).
pub const MAX_DOWNLOAD_BYTES: u64 = 512 << 20;
/// Max release-metadata body (Go parity, also in github.rs).
pub const MAX_RELEASE_METADATA_BYTES: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("fetch failed: {0}")]
    Fetch(#[from] FetchError),
    #[error("no binary available for this platform")]
    NoAsset,
    #[error("unsafe release asset name {0:?}")]
    UnsafeAssetName(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("size mismatch: expected {expected}, got {got}")]
    SizeMismatch { expected: i64, got: i64 },
    #[error("download size {0} exceeds maximum {MAX_DOWNLOAD_BYTES}")]
    TooLarge(i64),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("no checksum found for {0}")]
    NoChecksum(String),
    #[error("invalid checksum {0:?}: {1}")]
    BadChecksum(String, String),
    #[error("extraction failed: {0}")]
    Extract(String),
    #[error("new binary verification failed: {0}")]
    Verify(String),
    #[error("cannot determine binary path: {0}")]
    BinaryPath(String),
    #[error("no write permission to {0} (try running with sudo)")]
    NoPermission(String),
    #[error("backup failed: {0}")]
    Backup(String),
    #[error("installation failed: {0}")]
    Install(String),
    #[error("no backup found at {0}")]
    NoBackup(String),
    #[error("rollback failed: {0}")]
    Rollback(String),
    #[error("io error: {0}")]
    Io(String),
}

/// Outcome of a successful update.
#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub old_version: String,
    pub new_version: String,
    pub backup_path: Option<String>,
    pub message: String,
}

/// Path of the currently running binary (resolves symlinks like Go).
pub fn current_binary_path() -> Result<PathBuf, UpdateError> {
    let exe = std::env::current_exe().map_err(|e| UpdateError::BinaryPath(e.to_string()))?;
    exe.canonicalize()
        .map_err(|e| UpdateError::BinaryPath(e.to_string()))
}

/// Backup path for a binary (`<bin>.backup`, Go parity).
pub fn backup_path(binary: &Path) -> PathBuf {
    let mut s = binary.as_os_str().to_owned();
    s.push(".backup");
    PathBuf::from(s)
}

/// Reject empty / `.` / `..` / path-traversal asset names (Go `safeAssetName`).
pub fn safe_asset_name(name: &str) -> Result<&str, UpdateError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || Path::new(name)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            != Some(name.to_string())
    {
        return Err(UpdateError::UnsafeAssetName(name.to_string()));
    }
    Ok(name)
}

/// Parse a `checksums.txt`-style file into filename → sha256.
/// Mirrors Go `parseChecksums`: skips blanks/invalid, rejects duplicates.
pub fn parse_checksums(data: &str) -> Result<std::collections::HashMap<String, String>, UpdateError> {
    use std::collections::HashMap;
    let mut out = HashMap::new();
    for line in data.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        // Format: "<sha256> <whitespace> <filename>"
        let mut parts = line.splitn(2, |c: char| c.is_whitespace());
        let raw_hash = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();
        if raw_hash.len() != 64 || !raw_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if rest.is_empty() {
            continue;
        }
        let filename = rest
            .trim_start_matches(['*', ' '])
            .trim()
            .to_string();
        if filename.is_empty() {
            continue;
        }
        if out.contains_key(&filename) {
            return Err(UpdateError::BadChecksum(
                filename,
                "duplicate checksum entry".into(),
            ));
        }
        out.insert(filename, raw_hash.to_lowercase());
    }
    Ok(out)
}

/// Verify the SHA-256 of a file against `expected` hex.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<(), UpdateError> {
    let expected = expected.trim().to_lowercase();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::BadChecksum(
            expected,
            "invalid sha256 length/charset".into(),
        ));
    }
    let mut f = std::fs::File::open(path).map_err(|e| UpdateError::Io(e.to_string()))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| UpdateError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let actual = hex_encode(h.finalize());
    if actual != expected {
        return Err(UpdateError::ChecksumMismatch { expected, actual });
    }
    Ok(())
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Download `url` to `dest`, enforcing `expected_size` when > 0.
pub fn download_file(url: &str, dest: &Path, expected_size: i64) -> Result<(), UpdateError> {
    if expected_size < 0 {
        return Err(UpdateError::Download(format!(
            "download size cannot be negative: {expected_size}"
        )));
    }
    if expected_size as u64 > MAX_DOWNLOAD_BYTES {
        return Err(UpdateError::TooLarge(expected_size));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .redirect_auth_headers(ureq::config::RedirectAuthHeaders::Never)
        .http_status_as_error(false)
        .build()
        .into();
    let resp = agent
        .get(url)
        .header("User-Agent", "bvr-updater")
        .call()
        .map_err(|e| UpdateError::Download(e.to_string()))?;
    if resp.status().as_u16() != 200 {
        return Err(UpdateError::Download(format!(
            "download returned status: {}",
            resp.status()
        )));
    }
    // Go parity: reject early when the Content-Length header disagrees.
    if expected_size > 0 {
        if let Some(len) = resp.headers().get("content-length") {
            if let Ok(s) = len.to_str() {
                if let Ok(header_len) = s.trim().parse::<i64>() {
                    if header_len != expected_size {
                        return Err(UpdateError::SizeMismatch {
                            expected: expected_size,
                            got: header_len,
                        });
                    }
                }
            }
        }
    }
    let out = std::fs::File::create(dest).map_err(|e| UpdateError::Io(e.to_string()))?;
    let mut out = std::io::BufWriter::new(out);
    let limit: u64 = if expected_size > 0 {
        expected_size as u64 + 1
    } else {
        MAX_DOWNLOAD_BYTES + 1
    };
    let n = std::io::copy(&mut resp.into_body().into_reader().take(limit), &mut out)
        .map_err(|e| UpdateError::Download(format!("failed to write file: {e}")))?;
    out.flush().map_err(|e| UpdateError::Io(e.to_string()))?;
    if expected_size > 0 && n != expected_size as u64 {
        return Err(UpdateError::SizeMismatch {
            expected: expected_size,
            got: n as i64,
        });
    }
    if expected_size == 0 && n > MAX_DOWNLOAD_BYTES {
        return Err(UpdateError::TooLarge(n as i64));
    }
    Ok(())
}

/// Extract the `bvr` binary from a `.tar.gz` or `.zip` archive.
pub fn extract_binary(archive: &Path, dest: &Path) -> Result<(), UpdateError> {
    let is_zip = archive
        .extension()
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);
    if is_zip {
        extract_from_zip(archive, dest)
    } else {
        extract_from_tar_gz(archive, dest)
    }
}

fn binary_name_in_archive(name: &str) -> bool {
    let base = Path::new(name)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    base == BINARY_NAME || base == format!("{BINARY_NAME}.exe")
}

fn write_bounded(src: impl Read, dest: &Path) -> Result<(), UpdateError> {
    let mut src = src.take(MAX_DOWNLOAD_BYTES + 1);
    let out = std::fs::File::create(dest).map_err(|e| UpdateError::Extract(e.to_string()))?;
    let mut out = std::io::BufWriter::new(out);
    let n = std::io::copy(&mut src, &mut out).map_err(|e| UpdateError::Extract(e.to_string()))?;
    out.flush().map_err(|e| UpdateError::Extract(e.to_string()))?;
    if n > MAX_DOWNLOAD_BYTES {
        return Err(UpdateError::Extract(format!(
            "extracted binary exceeds maximum size {MAX_DOWNLOAD_BYTES}"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

fn extract_from_tar_gz(archive: &Path, dest: &Path) -> Result<(), UpdateError> {
    let f = std::fs::File::open(archive).map_err(|e| UpdateError::Extract(e.to_string()))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut tar = tar::Archive::new(gz);
    let entries = tar.entries().map_err(|e| UpdateError::Extract(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| UpdateError::Extract(e.to_string()))?;
        let path = entry.path().map_err(|e| UpdateError::Extract(e.to_string()))?;
        let name = path.to_string_lossy().into_owned();
        if !binary_name_in_archive(&name) {
            continue;
        }
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let size = entry.header().size().map_err(|e| UpdateError::Extract(e.to_string()))?;
        if size > MAX_DOWNLOAD_BYTES {
            return Err(UpdateError::Extract(format!(
                "extracted binary size {size} exceeds maximum {MAX_DOWNLOAD_BYTES}"
            )));
        }
        return write_bounded(entry, dest);
    }
    Err(UpdateError::Extract("binary not found in archive".into()))
}

fn extract_from_zip(archive: &Path, dest: &Path) -> Result<(), UpdateError> {
    let f = std::fs::File::open(archive).map_err(|e| UpdateError::Extract(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| UpdateError::Extract(e.to_string()))?;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| UpdateError::Extract(e.to_string()))?;
        let name = entry.name().to_string();
        if !binary_name_in_archive(&name) || entry.is_dir() {
            continue;
        }
        if entry.size() > MAX_DOWNLOAD_BYTES {
            return Err(UpdateError::Extract(format!(
                "extracted binary size {} exceeds maximum {MAX_DOWNLOAD_BYTES}",
                entry.size()
            )));
        }
        return write_bounded(entry, dest);
    }
    Err(UpdateError::Extract("binary not found in archive".into()))
}

/// Check the install directory is writable (Go: `.bvr-update-test` probe).
fn check_writable(binary_path: &Path) -> Result<(), UpdateError> {
    let dir = binary_path.parent().unwrap_or(Path::new("."));
    let probe = dir.join(".bvr-update-test");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(UpdateError::NoPermission(dir.display().to_string())),
    }
}

fn copy_file(src: &Path, dst: &Path) -> Result<(), UpdateError> {
    std::fs::copy(src, dst).map_err(|e| UpdateError::Io(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Download + verify + install `release` over the running binary.
/// `progress` receives human-readable lines (Go's `fmt.Println` steps).
pub fn perform_update(
    release: &Release,
    progress: &dyn Fn(&str),
) -> Result<UpdateResult, UpdateError> {
    let old = crate::version::current_version();
    if crate::version::compare_versions(&release.tag_name, &old) <= 0 {
        return Ok(UpdateResult {
            message: format!("Already at version {old} (latest: {})", release.tag_name),
            old_version: old,
            new_version: release.tag_name.clone(),
            backup_path: None,
        });
    }
    let asset = release.find_platform_asset().ok_or(UpdateError::NoAsset)?;
    let binary_path = current_binary_path()?;
    check_writable(&binary_path)?;

    let tmp = std::env::temp_dir().join(format!("bvr-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| UpdateError::Io(e.to_string()))?;
    let cleanup_tmp = || {
        let _ = std::fs::remove_dir_all(&tmp);
    };

    // Checksums sidecar (optional, like Go).
    let mut checksums = std::collections::HashMap::new();
    if let Some(sum_asset) = release.find_checksum_asset() {
        let sum_path = tmp.join("checksums.txt");
        if let Err(e) = download_file(&sum_asset.browser_download_url, &sum_path, sum_asset.size) {
            cleanup_tmp();
            return Err(UpdateError::Download(format!("checksum download failed: {e}")));
        }
        let data =
            std::fs::read_to_string(&sum_path).map_err(|e| UpdateError::Io(e.to_string()))?;
        // `.sha256` sidecar is "<hash>  <archive>"; `checksums.txt` is multi-line.
        // Both parse through the same parser; single-line works fine.
        match parse_checksums(&data) {
            Ok(m) => checksums = m,
            Err(e) => {
                cleanup_tmp();
                return Err(e);
            }
        }
    }

    let asset_name = safe_asset_name(&asset.name)?.to_string();
    let archive_path = tmp.join(&asset_name);
    progress(&format!("Downloading {}...", release.tag_name));
    if let Err(e) = download_file(&asset.browser_download_url, &archive_path, asset.size) {
        cleanup_tmp();
        return Err(UpdateError::Download(format!("download failed: {e}")));
    }
    if !checksums.is_empty() {
        let expected = checksums.get(&asset.name).ok_or_else(|| {
            cleanup_tmp();
            UpdateError::NoChecksum(asset.name.clone())
        })?;
        progress("Verifying checksum...");
        if let Err(e) = verify_checksum(&archive_path, expected) {
            cleanup_tmp();
            return Err(UpdateError::Download(format!(
                "checksum verification failed: {e}"
            )));
        }
    }

    let mut new_bin = tmp.join(format!("{BINARY_NAME}-new"));
    if cfg!(target_os = "windows") {
        new_bin.set_extension("exe");
    }
    progress("Extracting...");
    if let Err(e) = extract_binary(&archive_path, &new_bin) {
        cleanup_tmp();
        return Err(UpdateError::Extract(format!("extraction failed: {e}")));
    }

    // Verify the new binary runs.
    progress("Verifying new binary...");
    match std::process::Command::new(&new_bin).arg("--version").output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            cleanup_tmp();
            return Err(UpdateError::Verify(format!(
                "exit {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Err(e) => {
            cleanup_tmp();
            return Err(UpdateError::Verify(e.to_string()));
        }
    }

    // Backup: rename current out of the way (avoids ETXTBSY / file-in-use),
    // fall back to copy so an existing backup is never destroyed first.
    let backup = backup_path(&binary_path);
    progress(&format!("Backing up current binary to {}...", backup.display()));
    let mut moved_for_backup = false;
    if std::fs::rename(&binary_path, &backup).is_ok() {
        moved_for_backup = true;
    } else if copy_file(&binary_path, &backup).is_err() {
        cleanup_tmp();
        return Err(UpdateError::Backup("backup failed".into()));
    }

    // Install.
    progress("Installing new version...");
    if std::fs::rename(&new_bin, &binary_path).is_err() {
        if !moved_for_backup {
            let _ = std::fs::remove_file(&binary_path);
        }
        if copy_file(&new_bin, &binary_path).is_err() {
            // Restore from backup.
            let restored = std::fs::rename(&backup, &binary_path).is_ok()
                || copy_file(&backup, &binary_path).is_ok();
            cleanup_tmp();
            if !restored {
                return Err(UpdateError::Install(format!(
                    "installation failed (restore also failed; manual recovery: mv {} {})",
                    backup.display(),
                    binary_path.display()
                )));
            }
            return Err(UpdateError::Install("installation failed (restored from backup)".into()));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if let Err(e) = std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)) {
            eprintln!("Warning: could not set permissions: {e}");
        }
    }
    cleanup_tmp();
    let message = format!("Successfully updated from {old} to {}", release.tag_name);
    Ok(UpdateResult {
        old_version: old,
        new_version: release.tag_name.clone(),
        backup_path: Some(backup.display().to_string()),
        message,
    })
}

/// Restore the previous binary from `<bin>.backup` (Go `Rollback`).
pub fn perform_rollback() -> Result<(), UpdateError> {
    let binary_path = current_binary_path()?;
    let backup = backup_path(&binary_path);
    if !backup.exists() {
        return Err(UpdateError::NoBackup(backup.display().to_string()));
    }
    println!("Rolling back from backup at {}...", backup.display());
    let bad = {
        let mut s = binary_path.as_os_str().to_owned();
        s.push(".bad");
        PathBuf::from(s)
    };
    let _ = std::fs::remove_file(&bad);
    let moved_to_bad = std::fs::rename(&binary_path, &bad).is_ok();
    if std::fs::rename(&backup, &binary_path).is_err() {
        if !moved_to_bad {
            let _ = std::fs::remove_file(&binary_path);
        }
        if copy_file(&backup, &binary_path).is_err() {
            if moved_to_bad {
                let _ = std::fs::rename(&bad, &binary_path);
            }
            return Err(UpdateError::Rollback("rollback failed".into()));
        }
    }
    if moved_to_bad {
        let _ = std::fs::remove_file(&bad);
    }
    println!("Rollback complete");
    Ok(())
}

/// Download URL helpers (exported for `--update-dry-run` output).
pub fn releases_url() -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        assert!(safe_asset_name("bvr-v0.1.8-linux-x86_64.tar.gz").is_ok());
        assert!(safe_asset_name("").is_err());
        assert!(safe_asset_name("..").is_err());
        assert!(safe_asset_name("a/b").is_err());
    }

    #[test]
    fn checksums_parse() {
        let data = "abc123  file one.tar.gz\n\nDEF456 *file-two.zip\nnot-a-hash file\n";
        // abc123/DEF456 are not 64-hex so they are skipped
        let m = parse_checksums(data).unwrap();
        assert!(m.is_empty());
        let h = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let data = format!("{h}  bvr-v0.1.8-linux-x86_64.tar.gz\n");
        let m = parse_checksums(&data).unwrap();
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn extract_tar_gz_roundtrip() {
        let dir = std::env::temp_dir();
        let arch = dir.join(format!("bvr-arch-test-{}.tar.gz", std::process::id()));
        let dest = dir.join(format!("bvr-bin-test-{}", std::process::id()));
        {
            let f = std::fs::File::create(&arch).unwrap();
            let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            let payload = b"fake-bvr-binary";
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(payload.len() as u64);
            hdr.set_mode(0o755);
            hdr.set_cksum();
            tar.append_data(&mut hdr, "bvr", &payload[..]).unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        extract_binary(&arch, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"fake-bvr-binary");
        let _ = std::fs::remove_file(&arch);
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn extract_zip_roundtrip() {
        let dir = std::env::temp_dir();
        let arch = dir.join(format!("bvr-arch-test-{}.zip", std::process::id()));
        let dest = dir.join(format!("bvr-bin-zip-test-{}", std::process::id()));
        {
            let f = std::fs::File::create(&arch).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            zip.start_file("bvr", zip::write::SimpleFileOptions::default()).unwrap();
            use std::io::Write as _;
            zip.write_all(b"fake-bvr-binary").unwrap();
            zip.finish().unwrap();
        }
        extract_binary(&arch, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"fake-bvr-binary");
        let _ = std::fs::remove_file(&arch);
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn checksum_roundtrip() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("bvr-sum-test-{}", std::process::id()));
        std::fs::write(&p, b"hello").unwrap();
        let h = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_checksum(&p, h).unwrap();
        verify_checksum(&p, &h.to_uppercase()).unwrap();
        assert!(verify_checksum(&p, "00").is_err());
        let _ = std::fs::remove_file(&p);
    }
}

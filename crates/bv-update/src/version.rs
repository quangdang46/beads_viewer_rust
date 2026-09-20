//! Version resolution and comparison — port of Go `pkg/version` +
//! `compareVersions` in `pkg/updater/updater.go`.

/// Current binary version, normalized with a `v` prefix (e.g. `v0.1.7`).
/// Mirrors Go's `version.Version` (ldflags → buildinfo → hardcoded fallback);
/// for Rust the crate version is stamped at compile time so it is never empty.
pub fn current_version() -> String {
    let v = env!("CARGO_PKG_VERSION").trim();
    if v.strip_prefix('v').is_some() {
        v.to_string()
    } else {
        format!("v{v}")
    }
}

/// True when the version string marks a development build (`dev`, `dirty`,
/// `nightly`, `local`, `snapshot`, `git`, ...). Dev builds are treated as
/// newer than any release to avoid false "update available" prompts.
pub fn is_dev_version(v: &str) -> bool {
    let v = v.to_lowercase();
    ["dev", "dirty", "nightly", "local", "snapshot", "git"]
        .iter()
        .any(|m| v.contains(m))
}

/// Compare two semver-ish strings with optional leading `v` and optional
/// pre-release suffix. Returns 1 if `a>b`, -1 if `a<b`, 0 if equal.
/// Faithful port of Go `compareVersions` including the dev-build rules.
pub fn compare_versions(a: &str, b: &str) -> i32 {
    struct Parsed {
        parts: [i64; 3],
        prerelease: bool,
        pre_label: String,
    }

    fn parse(v: &str) -> Option<Parsed> {
        let v = v.trim();
        let v = v.strip_prefix('v').unwrap_or(v);
        let (core, pre) = match v.find('-') {
            Some(idx) => (&v[..idx], Some(v[idx + 1..].trim().to_string())),
            None => (v, None),
        };
        let mut parts = [0i64; 3];
        // Fail early when nothing is numeric (e.g. "dev").
        if !core.split('.').any(|p| p.parse::<i64>().is_ok()) {
            return None;
        }
        for (i, part) in core.split('.').take(3).enumerate() {
            match part.parse::<i64>() {
                Ok(n) => parts[i] = n,
                Err(_) => return None,
            }
        }
        Some(Parsed {
            parts,
            prerelease: pre.is_some(),
            pre_label: pre.unwrap_or_default(),
        })
    }

    fn is_dev_label(label: &str) -> bool {
        let l = label.to_lowercase();
        ["dev", "dirty", "nightly", "local", "snapshot", "git"]
            .iter()
            .any(|m| l.contains(m))
    }

    let p1 = parse(a);
    let p2 = parse(b);

    // Local (b) is an unparseable dev build → newer than anything.
    if p2.is_none() && is_dev_version(b) {
        return -1;
    }
    // Local has a dev-like prerelease suffix on the same base → no update.
    if let (Some(p1), Some(p2)) = (p1.as_ref(), p2.as_ref()) {
        if p2.prerelease && is_dev_label(&p2.pre_label) {
            let mut remote_higher = false;
            let mut local_higher = false;
            for i in 0..3 {
                if p1.parts[i] > p2.parts[i] {
                    remote_higher = true;
                    break;
                }
                if p1.parts[i] < p2.parts[i] {
                    local_higher = true;
                    break;
                }
            }
            if local_higher {
                return -1;
            }
            if !remote_higher && p1.parts == p2.parts {
                return -1;
            }
            if remote_higher {
                // remote genuinely higher — fall through to normal compare
            } else if !local_higher {
                // equal base handled above; unreachable, keep going
            }
        }
    }

    match (p1, p2) {
        (Some(_), None) => 1,
        (None, Some(_)) => -1,
        (None, None) => {
            // Both unparseable: deterministic lexicographic fallback.
            let x: &str = a.trim().strip_prefix('v').unwrap_or(a.trim());
            let y: &str = b.trim().strip_prefix('v').unwrap_or(b.trim());
            x.cmp(y) as i32
        }
        (Some(p1), Some(p2)) => {
            for i in 0..3 {
                if p1.parts[i] > p2.parts[i] {
                    return 1;
                }
                if p1.parts[i] < p2.parts[i] {
                    return -1;
                }
            }
            match (p1.prerelease, p2.prerelease) {
                (true, false) => return -1,
                (false, true) => return 1,
                (true, true) => {
                    let a_ids: Vec<&str> = p1.pre_label.split('.').collect();
                    let b_ids: Vec<&str> = p2.pre_label.split('.').collect();
                    let limit = a_ids.len().min(b_ids.len());
                    for i in 0..limit {
                        match (a_ids[i].parse::<i64>(), b_ids[i].parse::<i64>()) {
                            (Ok(n1), Ok(n2)) => {
                                if n1 != n2 {
                                    return if n1 > n2 { 1 } else { -1 };
                                }
                            }
                            (Err(_), Err(_)) => {
                                if a_ids[i] != b_ids[i] {
                                    return if a_ids[i] > b_ids[i] { 1 } else { -1 };
                                }
                            }
                            // SemVer: numeric < non-numeric.
                            (Ok(_), Err(_)) => return -1,
                            (Err(_), Ok(_)) => return 1,
                        }
                    }
                    if a_ids.len() != b_ids.len() {
                        return if a_ids.len() > b_ids.len() { 1 } else { -1 };
                    }
                }
                (false, false) => {}
            }
            0
        }
    }
}

/// True when `candidate` is newer than this binary's version.
pub fn is_newer_than_current(candidate: &str) -> bool {
    compare_versions(candidate, &current_version()) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ordering() {
        assert_eq!(compare_versions("v0.1.8", "v0.1.7"), 1);
        assert_eq!(compare_versions("v0.1.7", "v0.1.8"), -1);
        assert_eq!(compare_versions("v0.1.7", "v0.1.7"), 0);
        assert_eq!(compare_versions("0.1.8", "v0.1.7"), 1);
    }

    #[test]
    fn prerelease_lower_than_release() {
        assert_eq!(compare_versions("v1.2.3-alpha", "v1.2.3"), -1);
        assert_eq!(compare_versions("v1.2.3", "v1.2.3-alpha"), 1);
        assert_eq!(compare_versions("v1.2.3-alpha.1", "v1.2.3-alpha.2"), -1);
    }

    #[test]
    fn dev_build_never_prompts() {
        assert_eq!(compare_versions("v0.1.8", "dev"), -1);
        assert_eq!(compare_versions("v0.1.8", "v0.1.8-dirty"), -1);
    }
}

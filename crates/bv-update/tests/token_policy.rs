//! The ambient GitHub token must never leave the machine unless the user
//! opted in, and must never ride a redirect to a non-GitHub host.
//!
//! Oracle: `beads_viewer/pkg/updater/updater.go` at commit 18afafa —
//! `ambientGitHubToken` (203-208), `githubToken` (215-220), `isGitHubHost`
//! (225-229), `setGitHubAuth` (235-239), and `downloadFile`'s `CheckRedirect`
//! (969-982). The security property is Go's own: a credential that merely
//! happens to be in the environment is never transmitted, and even an opt-in
//! credential is scoped to GitHub hosts.

use bv_update::prefs::{
    ambient_github_token, github_auth_header, github_token, is_github_host, ENV_USE_TOKEN,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Blank every variable the resolver consults, then apply `pairs`. The
/// process environment is global, so every test that mutates it takes this
/// lock and restores the previous values on drop.
struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(pairs: &[(&'static str, &str)]) -> Self {
        let lock = ENV_LOCK.get_or_init(|| Mutex::new(()));
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let saved = pairs
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in pairs {
            // SAFETY: ENV_LOCK is held for the guard's whole lifetime and every
            // test that mutates the environment takes it.
            unsafe { std::env::set_var(k, v) };
        }
        Self {
            _lock: guard,
            saved,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            // SAFETY: still holding ENV_LOCK via self._lock.
            unsafe {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}

const RESOLVER_VARS: [&str; 4] = [
    "BV_UPDATE_USE_TOKEN",
    "BV_NO_UPDATE_CHECK",
    "GITHUB_TOKEN",
    "GH_TOKEN",
];

/// Blank every token-related variable and set `BV_NO_SAVED_CONFIG=1` so a
/// developer's real `~/.config/bv/config.yaml` cannot influence the result
/// (Go `savedConfigDisabled`, updater.go:95-97).
fn clean_env(pairs: &[(&'static str, &str)]) -> EnvGuard {
    let mut all: Vec<(&'static str, &str)> = RESOLVER_VARS.iter().map(|k| (*k, "")).collect();
    all.push(("BV_NO_SAVED_CONFIG", "1"));
    all.extend_from_slice(pairs);
    EnvGuard::set(&all)
}

#[test]
fn an_exported_token_is_never_transmitted_without_opt_in() {
    let _env = clean_env(&[("GITHUB_TOKEN", "ghp_developer_secret")]);

    // The credential is readable…
    assert_eq!(
        ambient_github_token().as_deref(),
        Some("ghp_developer_secret")
    );
    // …but nothing is authorized to send it.
    assert_eq!(github_token(), None);
    for url in [
        "https://api.github.com/repos/a/b/releases/latest",
        "https://github.com/a/b/releases/download/v1.2.3/bvr.tar.gz",
        "https://objects.githubusercontent.com/x",
    ] {
        assert_eq!(
            github_auth_header(url),
            None,
            "{url} must stay anonymous without an opt-in"
        );
    }
}

#[test]
fn the_env_opt_in_must_be_truthy() {
    // Each case gets its own scope: `EnvGuard` holds ENV_LOCK for its whole
    // lifetime and `std::sync::Mutex` is not reentrant, so a nested guard
    // would deadlock rather than nest.
    for (value, expected) in [
        ("0", None),
        ("false", None),
        ("no", None),
        ("off", None),
        ("1", Some("Bearer tok")),
    ] {
        let _env = clean_env(&[("GITHUB_TOKEN", "tok"), (ENV_USE_TOKEN, value)]);
        assert_eq!(
            github_auth_header("https://api.github.com/x").as_deref(),
            expected,
            "BV_UPDATE_USE_TOKEN={value:?}"
        );
    }
}

#[test]
fn an_opt_in_still_scopes_the_credential_to_github_hosts() {
    let _env = clean_env(&[("GITHUB_TOKEN", "tok"), (ENV_USE_TOKEN, "1")]);
    // Subdomains count, including the CDN asset host.
    for host in [
        "https://github.com/x",
        "https://api.github.com/x",
        "https://objects.githubusercontent.com/x",
        "https://raw.githubusercontent.com/x",
    ] {
        assert!(is_github_host(host), "{host} is a GitHub host");
        assert!(github_auth_header(host).is_some(), "{host} gets the header");
    }
    // Look-alike and third-party hosts do not.
    for host in [
        "https://cdn.example.com/x",
        "https://github.com.attacker.io/x",
        "https://notgithub.com/x",
        "https://raw.githubusercontent.com.evil.io/x",
    ] {
        assert!(!is_github_host(host), "{host} is not a GitHub host");
        assert_eq!(
            github_auth_header(host),
            None,
            "{host} must never receive the credential"
        );
    }
}

// --- redirect path ---------------------------------------------------------

/// Read a request head (everything up to the blank line) off a connection.
fn read_head(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) | Err(_) => break,
            Ok(_) => buf.push(byte[0]),
        }
        if buf.len() > 64 * 1024 {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn has_authorization(head: &str) -> bool {
    head.lines()
        .any(|l| l.to_ascii_lowercase().starts_with("authorization:"))
}

/// Prove the redirect guard the download paths rely on: a `302` from the
/// first hop must not carry `Authorization` to the second. Both hops are
/// loopback, so nothing leaves the machine and GitHub is never contacted.
#[test]
fn a_redirect_does_not_carry_the_authorization_header() {
    // Hop 2: records whether it saw the credential.
    let hop2 = TcpListener::bind("127.0.0.1:0").expect("bind hop2");
    let hop2_addr = hop2.local_addr().unwrap();
    let hop2_seen = std::sync::Arc::new(Mutex::new(None::<String>));
    let hop2_seen_for_thread = hop2_seen.clone();
    let hop2_thread = std::thread::spawn(move || {
        if let Ok((mut conn, _)) = hop2.accept() {
            let head = read_head(&mut conn);
            *hop2_seen_for_thread.lock().unwrap() = Some(head);
            let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        }
    });

    // Hop 1: asserts the credential *is* present, then redirects.
    let hop1 = TcpListener::bind("127.0.0.1:0").expect("bind hop1");
    let hop1_addr = hop1.local_addr().unwrap();
    let hop1_saw_auth = std::sync::Arc::new(Mutex::new(false));
    let hop1_flag = hop1_saw_auth.clone();
    let hop1_thread = std::thread::spawn(move || {
        if let Ok((mut conn, _)) = hop1.accept() {
            let head = read_head(&mut conn);
            *hop1_flag.lock().unwrap() = has_authorization(&head);
            let location = format!("Location: http://{hop2_addr}/asset\r\n");
            let _ = conn.write_all(
                format!("HTTP/1.1 302 Found\r\n{location}Content-Length: 0\r\n\r\n").as_bytes(),
            );
        }
    });

    // The exact agent configuration `download_file` builds: Authorization is
    // dropped on *every* redirect, which is stricter than Go's CheckRedirect
    // (Go deletes the header only when the redirect leaves GitHub hosts).
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .redirect_auth_headers(ureq::config::RedirectAuthHeaders::Never)
        .http_status_as_error(false)
        .build()
        .into();
    let response = agent
        .get(&format!("http://{hop1_addr}/start"))
        .header("Authorization", "Bearer ghp_developer_secret")
        .call();
    assert!(response.is_ok(), "the redirect chain should be followed");

    hop1_thread.join().unwrap();
    hop2_thread.join().unwrap();

    assert!(
        *hop1_saw_auth.lock().unwrap(),
        "the first request must carry the credential (sanity: the test is wired up)"
    );
    let hop2_head = hop2_seen.lock().unwrap().clone().expect("hop2 was reached");
    assert!(
        !has_authorization(&hop2_head),
        "Authorization leaked across the redirect; second hop saw:\n{hop2_head}"
    );
}

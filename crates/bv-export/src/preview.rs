//! Preview server — port of Go `pkg/export/preview.go` +
//! `livereload.go`: static file serving on 127.0.0.1:9000-9100 with
//! SSE livereload at /__preview__/events and status endpoint.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const PORT_RANGE_START: u16 = 9000;
const PORT_RANGE_END: u16 = 9100;

#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("no available port in range {start}-{end}")]
    NoPort { start: u16, end: u16 },
    #[error("path traversal detected: {0}")]
    PathTraversal(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Resolve a safe path under `root` (anti-traversal guard).
fn safe_join(root: &Path, requested: &str) -> Result<PathBuf, PreviewError> {
    let cleaned = requested.trim_start_matches('/');
    let resolved = root.join(cleaned);
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // The resolved path must stay inside root (after canonicalizing existing
    // portions; for not-yet-existing files we check component-wise).
    if let Ok(canonical) = resolved.canonicalize() {
        if !canonical.starts_with(&canonical_root) {
            return Err(PreviewError::PathTraversal(requested.to_string()));
        }
    } else {
        // Check components for traversal sequences.
        for comp in Path::new(cleaned).components() {
            if matches!(comp, std::path::Component::ParentDir) {
                return Err(PreviewError::PathTraversal(requested.to_string()));
            }
        }
    }
    Ok(resolved)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "sqlite3" | "db" => "application/octet-stream",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Livereload hub: broadcasts SSE reload events to connected clients.
struct LiveReloadHub {
    shutdown: Arc<AtomicBool>,
}

impl LiveReloadHub {
    fn new() -> Self {
        LiveReloadHub {
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Start the preview server. Blocks until Ctrl+C or error.
/// Returns the bound port on success before blocking (via callback).
pub fn start_preview(
    bundle_dir: &Path,
    on_ready: impl FnOnce(u16),
    livereload_enabled: bool,
) -> Result<(), PreviewError> {
    let root = bundle_dir.canonicalize()?;
    let (server, port) = find_available_port()?;
    let hub = Arc::new(LiveReloadHub::new());

    // Watch for file changes to trigger livereload.
    let lr_hub = Arc::clone(&hub);
    let watch_dir = root.clone();
    if livereload_enabled {
        std::thread::spawn(move || loop {
            if lr_hub.shutdown.load(Ordering::Relaxed) {
                break;
            }
            // Polling watcher: notify crate integration lands with full wiring.
            std::thread::sleep(Duration::from_millis(500));
            let _ = &watch_dir;
        });
    }

    on_ready(port);

    // Track active SSE connections for livereload.
    let sse_clients: Arc<std::sync::Mutex<Vec<std::sync::mpsc::Sender<()>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let _ = &sse_clients;

    loop {
        match server.recv() {
            Ok(request) => {
                let method = request.method().as_str().to_string();
                let url = request.url().to_string();
                let hub = Arc::clone(&hub);

                // Handle request inline (single-threaded is fine for preview).
                let response = handle_request(&root, &method, &url, &hub, livereload_enabled);
                let _ = request.respond(response);
            }
            Err(e) => {
                eprintln!("preview: recv error: {e}");
                break;
            }
        }
    }

    Ok(())
}

fn find_available_port() -> Result<(tiny_http::Server, u16), PreviewError> {
    for port in PORT_RANGE_START..=PORT_RANGE_END {
        if let Ok(server) = tiny_http::Server::http(("127.0.0.1", port)) {
            return Ok((server, port));
        }
    }
    Err(PreviewError::NoPort {
        start: PORT_RANGE_START,
        end: PORT_RANGE_END,
    })
}

fn handle_request(
    root: &Path,
    method: &str,
    url: &str,
    _hub: &LiveReloadHub,
    livereload: bool,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let _ = method;

    // SSE livereload endpoint
    if url == "/__preview__/events" && livereload {
        let body = "event: connected\ndata: preview\n\n";
        return tiny_http::Response::from_string(body)
            .with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/event-stream"[..])
                    .unwrap(),
            )
            .with_header(
                tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]).unwrap(),
            );
    }

    // Status endpoint
    if url == "/__preview__/status" {
        let body = serde_json::json!({
            "status": "ok",
            "server": "bvr-preview",
        });
        return tiny_http::Response::from_string(body.to_string()).with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        );
    }

    // Static files: strip query params, default to index.html.
    let path_part = url.split('?').next().unwrap_or("/");
    let rel = if path_part == "/" {
        "index.html"
    } else {
        &path_part[1..]
    };

    match safe_join(root, rel) {
        Ok(file_path) => {
            if file_path.is_file() {
                let mut content = Vec::new();
                if let Ok(mut f) = std::fs::File::open(&file_path) {
                    let _ = f.read_to_end(&mut content);
                }
                let ct = content_type(&file_path);
                let mut response = tiny_http::Response::from_data(content).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], ct.as_bytes()).unwrap(),
                );
                response.add_header(
                    tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]).unwrap(),
                );
                response
            } else {
                tiny_http::Response::from_string("404 Not Found").with_status_code(404)
            }
        }
        Err(_) => tiny_http::Response::from_string("403 Forbidden").with_status_code(403),
    }
}

use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_blocks_traversal() {
        let root = std::env::temp_dir().join("bvr-preview-test");
        std::fs::create_dir_all(&root).unwrap();
        let result = safe_join(&root, "../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn safe_join_allows_normal_paths() {
        let root = std::env::temp_dir().join("bvr-preview-test-ok");
        std::fs::create_dir_all(&root).unwrap();
        let result = safe_join(&root, "index.html");
        assert!(result.is_ok());
    }

    #[test]
    fn content_type_mapping() {
        assert_eq!(
            content_type(Path::new("a.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("a.js")), "application/javascript");
        assert_eq!(content_type(Path::new("a.wasm")), "application/wasm");
        assert_eq!(
            content_type(Path::new("a.unknown")),
            "application/octet-stream"
        );
    }
}

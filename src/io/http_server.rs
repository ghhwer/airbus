//! Minimal HTTP server: static files from a resources dir + JSON-RPC at POST /rpc.

use super::{log, net};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Accept HTTP connections. `POST /rpc` → `on_rpc`; `GET` → files under `resources`.
pub fn serve_http<F>(host: &str, port: u16, resources: &Path, mut on_rpc: F) -> Result<(), String>
where
    F: FnMut(&[u8]) -> String,
{
    let resources = fs::canonicalize(resources).map_err(|e| {
        format!(
            "resources path {}: {e}",
            resources.display()
        )
    })?;
    if !resources.is_dir() {
        return Err(format!(
            "resources path is not a directory: {}",
            resources.display()
        ));
    }

    let listener = net::BoundListener::bind(host, port).map_err(|e| e.to_string())?;
    log::info(&format!(
        "http listening on {}:{} (resources {})",
        listener.host(),
        listener.port(),
        resources.display()
    ));

    loop {
        let mut stream = match listener.accept() {
            Ok(s) => s,
            Err(e) => {
                log::error(&e.to_string());
                continue;
            }
        };
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
        if let Err(e) = handle_connection(&mut stream, &resources, &mut on_rpc) {
            log::error(&e);
        }
    }
}

fn handle_connection<F>(
    stream: &mut std::net::TcpStream,
    resources: &Path,
    on_rpc: &mut F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> String,
{
    let request = read_http_request(stream).map_err(|e| e.to_string())?;
    let response = dispatch(&request, resources, on_rpc);
    stream
        .write_all(&response)
        .map_err(|e| e.to_string())?;
    Ok(())
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut std::net::TcpStream) -> std::io::Result<HttpRequest> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(header_end) = find_header_end(&buf) {
            let (header_bytes, rest) = buf.split_at(header_end);
            let headers_text = String::from_utf8_lossy(header_bytes);
            let mut lines = headers_text.split("\r\n");
            let request_line = lines.next().unwrap_or("");
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or("GET").to_string();
            let path = parts.next().unwrap_or("/").to_string();

            let mut content_length = 0usize;
            for line in lines {
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
            }

            let mut body = rest.to_vec();
            while body.len() < content_length {
                let n = stream.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            body.truncate(content_length);
            return Ok(HttpRequest { method, path, body });
        }
        if buf.len() > 64 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP headers too large",
            ));
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "incomplete HTTP request",
    ))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
}

fn dispatch<F>(request: &HttpRequest, resources: &Path, on_rpc: &mut F) -> Vec<u8>
where
    F: FnMut(&[u8]) -> String,
{
    let path = request.path.split('?').next().unwrap_or("/");
    match (request.method.as_str(), path) {
        ("POST", "/rpc") => {
            let out = on_rpc(&request.body);
            if out.is_empty() {
                // JSON-RPC notification: empty body with 204
                http_response(204, "No Content", "text/plain; charset=utf-8", b"")
            } else {
                http_response(
                    200,
                    "OK",
                    "application/json; charset=utf-8",
                    out.as_bytes(),
                )
            }
        }
        ("GET", _) | ("HEAD", _) => serve_static(resources, path, request.method == "HEAD"),
        _ => http_response(
            405,
            "Method Not Allowed",
            "text/plain; charset=utf-8",
            b"method not allowed\n",
        ),
    }
}

fn serve_static(resources: &Path, url_path: &str, head_only: bool) -> Vec<u8> {
    let rel = if url_path == "/" || url_path.is_empty() {
        "index.html"
    } else {
        url_path.trim_start_matches('/')
    };

    let Some(candidate) = safe_join(resources, rel) else {
        return http_response(403, "Forbidden", "text/plain; charset=utf-8", b"forbidden\n");
    };

    match fs::read(&candidate) {
        Ok(bytes) => {
            let ctype = content_type(&candidate);
            if head_only {
                http_response_headers_only(200, "OK", ctype, bytes.len())
            } else {
                http_response(200, "OK", ctype, &bytes)
            }
        }
        Err(_) => http_response(404, "Not Found", "text/plain; charset=utf-8", b"not found\n"),
    }
}

/// Resolve `rel` under `root`, rejecting `..` and escapes outside `root`.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut joined = PathBuf::new();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(s) => joined.push(s),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let full = root.join(&joined);
    if full.exists() {
        let canon = fs::canonicalize(&full).ok()?;
        if canon.starts_with(root) {
            Some(canon)
        } else {
            None
        }
    } else {
        Some(full)
    }
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn http_response(status: u16, reason: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn http_response_headers_only(
    status: u16,
    reason: &str,
    content_type: &str,
    content_length: usize,
) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n"
    )
    .into_bytes()
}

/// Handle a single already-parsed request (for unit tests).
pub fn handle_request_for_test<F>(
    method: &str,
    path: &str,
    body: &[u8],
    resources: &Path,
    mut on_rpc: F,
) -> Vec<u8>
where
    F: FnMut(&[u8]) -> String,
{
    let request = HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        body: body.to_vec(),
    };
    dispatch(&request, resources, &mut on_rpc)
}

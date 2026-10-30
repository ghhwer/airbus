//! Resolve the Airbus TCP endpoint from the environment.
//!
//! Precedence: `AIRBUS_URL` (`host:port`), else `AIRBUS_HOST` / `AIRBUS_PORT`,
//! else `127.0.0.1:9097`.
//!
//! Listen-host resolution for push callbacks: `AIRBUS_LISTEN_HOST` or an explicit
//! override wins; otherwise the local egress IP toward the Airbus peer is detected
//! (UDP `connect` + `local_addr`). Loopback peers keep `127.0.0.1`.

use std::env;
use std::net::{SocketAddr, UdpSocket};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 9097;

/// Parse `host:port` into a host/port pair.
pub fn parse_endpoint(url: &str) -> Result<(String, u16), String> {
    let (host, port_s) = url
        .rsplit_once(':')
        .ok_or_else(|| format!("Airbus endpoint must be host:port (got {url:?}); example: 127.0.0.1:9097"))?;
    if host.is_empty() {
        return Err(format!(
            "Airbus endpoint must be host:port (got {url:?}); example: 127.0.0.1:9097"
        ));
    }
    let port: u16 = port_s.parse().map_err(|_| {
        format!("Airbus endpoint must be host:port (got {url:?}); example: 127.0.0.1:9097")
    })?;
    Ok((host.to_string(), port))
}

/// Resolve host/port from `AIRBUS_URL` or `AIRBUS_HOST` / `AIRBUS_PORT`.
pub fn airbus_endpoint() -> Result<(String, u16), String> {
    if let Ok(url) = env::var("AIRBUS_URL") {
        let url = url.trim();
        if !url.is_empty() {
            return parse_endpoint(url);
        }
    }
    let host = env::var("AIRBUS_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let port = env::var("AIRBUS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    Ok((host, port))
}

/// Return `host:port` for the resolved endpoint.
pub fn airbus_url() -> Result<String, String> {
    let (host, port) = airbus_endpoint()?;
    Ok(format!("{host}:{port}"))
}

/// True when `host` is a conventional IPv4/IPv6 loopback name or address.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
    matches!(h.as_str(), "localhost" | "127.0.0.1" | "::1") || h.starts_with("127.")
}

/// Return the local IPv4 address used toward `peer_host:peer_port`.
///
/// Uses a UDP `connect` (no packets sent) then `local_addr()`, matching the
/// usual OS routing-table egress discovery trick.
pub fn detect_egress_ip(peer_host: &str, peer_port: u16) -> Result<String, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| {
        format!(
            "could not detect egress IP toward {peer_host}:{peer_port}: {e}; \
             set AIRBUS_LISTEN_HOST or pass with_listen_host(...)"
        )
    })?;
    socket.connect((peer_host, peer_port)).map_err(|e| {
        format!(
            "could not detect egress IP toward {peer_host}:{peer_port}: {e}; \
             set AIRBUS_LISTEN_HOST or pass with_listen_host(...)"
        )
    })?;
    let addr = socket.local_addr().map_err(|e| {
        format!(
            "could not detect egress IP toward {peer_host}:{peer_port}: {e}; \
             set AIRBUS_LISTEN_HOST or pass with_listen_host(...)"
        )
    })?;
    let ip = match addr {
        SocketAddr::V4(v4) => v4.ip().to_string(),
        SocketAddr::V6(v6) => v6.ip().to_string(),
    };
    if ip.is_empty() || ip == "0.0.0.0" || ip == "::" {
        return Err(format!(
            "egress IP toward {peer_host}:{peer_port} is unspecified ({ip:?}); \
             set AIRBUS_LISTEN_HOST or pass with_listen_host(...)"
        ));
    }
    Ok(ip)
}

/// Pick the host the daemon should dial back to for `on_event`.
///
/// Precedence:
/// 1. Explicit `override` (unless empty / `"auto"`)
/// 2. `AIRBUS_LISTEN_HOST` env
/// 3. `127.0.0.1` when the Airbus peer is loopback
/// 4. Detected egress IP toward the peer
///
/// Errors if a remote peer would be registered with a loopback listen host.
pub fn resolve_listen_host(
    peer_host: &str,
    peer_port: u16,
    override_host: Option<&str>,
) -> Result<String, String> {
    let mut chosen: Option<String> = None;
    if let Some(override_host) = override_host {
        let trimmed = override_host.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto") {
            chosen = Some(trimmed.to_string());
        }
    }
    if chosen.is_none() {
        if let Ok(env_host) = env::var("AIRBUS_LISTEN_HOST") {
            let trimmed = env_host.trim();
            if !trimmed.is_empty() {
                chosen = Some(trimmed.to_string());
            }
        }
    }
    let chosen = match chosen {
        Some(host) => host,
        None if is_loopback_host(peer_host) => return Ok("127.0.0.1".into()),
        None => detect_egress_ip(peer_host, peer_port)?,
    };

    if is_loopback_host(&chosen) && !is_loopback_host(peer_host) {
        return Err(format!(
            "listen host {chosen:?} is loopback but Airbus peer \
             {peer_host}:{peer_port} is remote; the daemon cannot deliver \
             on_event callbacks. Set AIRBUS_LISTEN_HOST or pass a routable \
             listen_host / with_listen_host(...)."
        ));
    }
    Ok(chosen)
}

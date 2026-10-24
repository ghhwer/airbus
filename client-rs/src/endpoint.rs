//! Resolve the Airbus TCP endpoint from the environment.
//!
//! Precedence: `AIRBUS_URL` (`host:port`), else `AIRBUS_HOST` / `AIRBUS_PORT`,
//! else `127.0.0.1:9097`.

use std::env;

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

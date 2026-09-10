use super::{log, net};

/// Parse `host:port` or bare `port` (host defaults to `127.0.0.1`).
pub fn parse_listen(spec: &str) -> Result<(String, u16), String> {
    match spec.rfind(':') {
        None => {
            let port: u16 = spec
                .parse()
                .map_err(|_| format!("invalid port: {spec}"))?;
            Ok(("127.0.0.1".to_string(), port))
        }
        Some(colon) => {
            let mut host = spec[..colon].to_string();
            if host.is_empty() {
                host = "127.0.0.1".to_string();
            }
            let port: u16 = spec[colon + 1..]
                .parse()
                .map_err(|_| format!("invalid port in listen spec: {spec}"))?;
            Ok((host, port))
        }
    }
}

/// Accept TCP connections and move raw bytes through `on_bytes`.
/// Empty response means no reply. Connection GC belongs here when added.
pub fn serve_tcp<F>(host: &str, port: u16, mut on_bytes: F) -> Result<(), String>
where
    F: FnMut(&[u8]) -> String,
{
    let listener = net::BoundListener::bind(host, port).map_err(|e| e.to_string())?;
    log::info(&format!("listening on {}:{}", listener.host(), listener.port()));
    loop {
        let mut conn = match listener.accept() {
            Ok(c) => c,
            Err(e) => {
                log::error(&e.to_string());
                continue;
            }
        };
        match net::recv_all(&mut conn) {
            Ok(input) if input.is_empty() => {}
            Ok(input) => {
                let out = on_bytes(&input);
                if !out.is_empty() {
                    if let Err(e) = net::send_all(&mut conn, out.as_bytes()) {
                        log::error(&e.to_string());
                    }
                }
            }
            Err(e) => log::error(&e.to_string()),
        }
    }
}

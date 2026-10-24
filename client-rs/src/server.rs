//! Spawn a disposable Airbus daemon for integration tests.

use crate::binary::airbus_bin;
use crate::rpc::RpcClient;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

const LISTEN_PREFIX: &str = "listening on ";

/// Guard that terminates the child process on drop.
pub struct ServerGuard {
    child: Child,
    pub client: RpcClient,
}

impl ServerGuard {
    pub fn client(&self) -> &RpcClient {
        &self.client
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Start Airbus on an ephemeral port and return an [`RpcClient`] pointed at it.
pub async fn start_server() -> Result<ServerGuard, Box<dyn std::error::Error + Send + Sync>> {
    start_server_with("127.0.0.1:0", Duration::from_secs(2)).await
}

pub async fn start_server_with(
    bind: &str,
    wait: Duration,
) -> Result<ServerGuard, Box<dyn std::error::Error + Send + Sync>> {
    let mut child = Command::new(airbus_bin())
        .args(["--listen", bind])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stderr = child.stderr.take().ok_or("missing stderr")?;
    let (host, port) = timeout(wait, wait_for_listen(stderr, &mut child))
        .await
        .map_err(|_| "timed out waiting for airbus listen line")??;

    Ok(ServerGuard {
        child,
        client: RpcClient::new(host, port).with_timeout(wait),
    })
}

async fn wait_for_listen(
    stderr: impl tokio::io::AsyncRead + Unpin,
    child: &mut Child,
) -> Result<(String, u16), Box<dyn std::error::Error + Send + Sync>> {
    let mut lines = BufReader::new(stderr).lines();
    let mut buf = String::new();
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(format!("airbus exited before listen ({status}): {buf}").into());
        }
        match lines.next_line().await? {
            Some(line) => {
                buf.push_str(&line);
                buf.push('\n');
                if let Some(rest) = line.strip_prefix(LISTEN_PREFIX).or_else(|| {
                    line.find(LISTEN_PREFIX)
                        .map(|i| &line[i + LISTEN_PREFIX.len()..])
                }) {
                    let rest = rest.trim();
                    if let Some((host, port_s)) = rest.rsplit_once(':') {
                        if let Ok(port) = port_s.parse::<u16>() {
                            return Ok((host.to_string(), port));
                        }
                    }
                }
            }
            None => {
                return Err(format!("timed out waiting for airbus listen line: {buf:?}").into());
            }
        }
    }
}

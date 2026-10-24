//! Resolve the Airbus daemon binary path for tests and helpers.

use std::env;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Path to the Airbus binary (`AIRBUS_BIN`, else `<repo>/out/airbus`).
pub fn airbus_bin() -> PathBuf {
    if let Ok(env) = env::var("AIRBUS_BIN") {
        return PathBuf::from(env);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("out")
        .join("airbus")
}

/// Run the Airbus binary with the given args (captures stdout/stderr).
pub async fn run_airbus(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new(airbus_bin())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
}

//! Unit tests for Airbus endpoint env resolution (no live server).

use airbus_client::endpoint::{airbus_endpoint, airbus_url, parse_endpoint};
use airbus_client::RpcClient;
use std::env;

fn clear_endpoint_env() {
    env::remove_var("AIRBUS_URL");
    env::remove_var("AIRBUS_HOST");
    env::remove_var("AIRBUS_PORT");
}

#[test]
fn airbus_endpoint_defaults() {
    clear_endpoint_env();
    assert_eq!(airbus_endpoint().unwrap(), ("127.0.0.1".into(), 9097));
    assert_eq!(airbus_url().unwrap(), "127.0.0.1:9097");
}

#[test]
fn airbus_endpoint_from_url() {
    clear_endpoint_env();
    env::set_var("AIRBUS_URL", "10.0.0.2:19097");
    assert_eq!(airbus_endpoint().unwrap(), ("10.0.0.2".into(), 19097));
    clear_endpoint_env();
}

#[test]
fn airbus_endpoint_from_host_port() {
    clear_endpoint_env();
    env::set_var("AIRBUS_HOST", "localhost");
    env::set_var("AIRBUS_PORT", "9098");
    assert_eq!(airbus_endpoint().unwrap(), ("localhost".into(), 9098));
    clear_endpoint_env();
}

#[test]
fn parse_endpoint_rejects_bad_url() {
    assert!(parse_endpoint("not-a-host-port").is_err());
}

#[test]
fn rpc_client_uses_env_defaults() {
    clear_endpoint_env();
    env::set_var("AIRBUS_URL", "127.0.0.1:9099");
    let client = RpcClient::from_env().unwrap().with_timeout(std::time::Duration::from_millis(1500));
    assert_eq!(client.host, "127.0.0.1");
    assert_eq!(client.port, 9099);
    clear_endpoint_env();
}

#[test]
fn rpc_client_from_url() {
    let client = RpcClient::from_url("10.1.2.3:4040")
        .unwrap()
        .with_timeout(std::time::Duration::from_secs(3));
    assert_eq!(client.host, "10.1.2.3");
    assert_eq!(client.port, 4040);
}

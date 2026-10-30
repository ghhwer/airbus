//! Unit tests for Airbus endpoint env resolution (no live server).

use airbus_client::endpoint::{
    airbus_endpoint, airbus_url, detect_egress_ip, is_loopback_host, parse_endpoint,
    resolve_listen_host,
};
use airbus_client::RpcClient;
use std::env;

fn clear_endpoint_env() {
    env::remove_var("AIRBUS_URL");
    env::remove_var("AIRBUS_HOST");
    env::remove_var("AIRBUS_PORT");
}

fn clear_listen_env() {
    env::remove_var("AIRBUS_LISTEN_HOST");
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
    let client = RpcClient::from_env()
        .unwrap()
        .with_timeout(std::time::Duration::from_millis(1500));
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

#[test]
fn loopback_host_detection() {
    assert!(is_loopback_host("127.0.0.1"));
    assert!(is_loopback_host("127.1.2.3"));
    assert!(is_loopback_host("localhost"));
    assert!(is_loopback_host("::1"));
    assert!(!is_loopback_host("10.0.0.1"));
    assert!(!is_loopback_host("prd-queue.internal"));
}

#[test]
fn resolve_listen_host_loopback_peer() {
    clear_listen_env();
    assert_eq!(
        resolve_listen_host("127.0.0.1", 9097, None).unwrap(),
        "127.0.0.1"
    );
    assert_eq!(
        resolve_listen_host("localhost", 9097, None).unwrap(),
        "127.0.0.1"
    );
}

#[test]
fn resolve_listen_host_env_override() {
    clear_listen_env();
    env::set_var("AIRBUS_LISTEN_HOST", "10.9.8.7");
    assert_eq!(
        resolve_listen_host("10.0.0.1", 9097, None).unwrap(),
        "10.9.8.7"
    );
    clear_listen_env();
}

#[test]
fn resolve_listen_host_explicit_wins_over_env() {
    clear_listen_env();
    env::set_var("AIRBUS_LISTEN_HOST", "10.9.8.7");
    assert_eq!(
        resolve_listen_host("10.0.0.1", 9097, Some("192.168.1.5")).unwrap(),
        "192.168.1.5"
    );
    assert_eq!(
        resolve_listen_host("10.0.0.1", 9097, Some("auto")).unwrap(),
        "10.9.8.7"
    );
    clear_listen_env();
}

#[test]
fn resolve_listen_host_refuses_loopback_for_remote() {
    clear_listen_env();
    let err = resolve_listen_host("10.0.0.1", 9097, Some("127.0.0.1")).unwrap_err();
    assert!(err.contains("loopback"), "{err}");
}

#[test]
fn detect_egress_ip_toward_loopback() {
    assert_eq!(detect_egress_ip("127.0.0.1", 9097).unwrap(), "127.0.0.1");
}

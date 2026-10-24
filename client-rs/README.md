# airbus-client

Async Tokio SDK for the [Airbus](https://github.com/ghhwer/airbus) JSON-RPC event bus.

```bash
cargo add airbus-client
```

```rust
use airbus_client::RpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = RpcClient::from_env()?;
    assert_eq!(client.ping().await?.as_str(), "pong");
    Ok(())
}
```

Endpoint defaults: `AIRBUS_URL` or `AIRBUS_HOST` / `AIRBUS_PORT` (else `127.0.0.1:9097`).

The daemon is distributed separately (container image or `cargo`/`make build`).
See the [repository README](https://github.com/ghhwer/airbus) for run instructions and
[docs/clients/rust.md](../docs/clients/rust.md) for usage (Queue, listen, batch).

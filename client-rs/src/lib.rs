//! Async Tokio client for the Airbus JSON-RPC event bus.
//!
//! Wire `*Params` / `*Result` types live under [`payloads`]. Envelope helpers are in
//! [`protocol`]. Endpoint defaults: `AIRBUS_URL` or `AIRBUS_HOST`/`AIRBUS_PORT` (else
//! `127.0.0.1:9097`).

pub mod binary;
pub mod endpoint;
pub mod payloads;
pub mod protocol;
pub mod queue;
pub mod rpc;
pub mod server;

pub use payloads::{DispatchStrategy, DuplexSide, ListenerEventParams, QueueMode};
pub use queue::Queue;
pub use rpc::{ClientError, EventListener, Pending, RpcClient};
pub use protocol::RpcError;
pub use server::{start_server, ServerGuard};

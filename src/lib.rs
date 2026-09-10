//! Airbus library: layered JSON-RPC over TCP with in-process event queues.
//!
//! Layers are composed, not subclassed. `AppService` has no TCP or JSON-RPC types;
//! `wiring` / `main` attach it to `RpcServer`.

pub mod app;
pub mod io;
pub mod proto;
pub mod runtime;
pub mod wiring;

pub use wiring::bind_app_service;

//! Composition: wire `AppService` methods onto an `RpcServer`.

use crate::app::{decode_params, AppService};
use crate::io::rpc_server::RpcServer;
use crate::proto::payloads::{AddParams, GetEventsParams, PostEventParams};
use crate::proto::rpc::Error;
use std::sync::Arc;

pub fn bind_app_service(server: &mut RpcServer, app: Arc<AppService>) {
    let ping_app = Arc::clone(&app);
    server.route("ping", move |_params| {
        Ok(serde_json::to_value(ping_app.ping()).expect("PingResult serializes"))
    });

    let add_app = Arc::clone(&app);
    server.route("add", move |params| {
        let typed: AddParams =
            decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(add_app.add(typed)).expect("AddResult serializes"))
    });

    let post_app = Arc::clone(&app);
    server.route("post_event", move |params| {
        let typed: PostEventParams =
            decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?;
        let result = post_app
            .post_event(typed)
            .map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(result).expect("PostEventResult serializes"))
    });

    let get_app = Arc::clone(&app);
    server.route("get_events", move |params| {
        let typed: GetEventsParams =
            decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(get_app.get_events(typed)).expect("GetEventsResult serializes"))
    });
}

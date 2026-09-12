//! Composition: wire `AppService` methods onto an `RpcServer`.

use crate::app::{decode_params, AppService};
use crate::io::rpc_server::RpcServer;
use crate::proto::payloads::{
    AddParams, AttachListenerParams, CreateQueueParams, DetachListenerParams,
    ListListenersParams, PeekEventsParams, PostEventParams,
};
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

    let create_app = Arc::clone(&app);
    server.route("create_queue", move |params| {
        let typed: CreateQueueParams =
            decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?;
        let result = create_app
            .create_queue(typed)
            .map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(result).expect("CreateQueueResult serializes"))
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

    let list_app = Arc::clone(&app);
    server.route("list_queues", move |_params| {
        Ok(serde_json::to_value(list_app.list_queues()).expect("ListQueuesResult serializes"))
    });

    let peek_app = Arc::clone(&app);
    server.route("peek_events", move |params| {
        let typed: PeekEventsParams =
            decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?;
        let result = peek_app
            .peek_events(typed)
            .map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(result).expect("PeekEventsResult serializes"))
    });

    let attach_app = Arc::clone(&app);
    server.route("attach_listener", move |params| {
        let typed: AttachListenerParams =
            decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?;
        let result = attach_app
            .attach_listener(typed)
            .map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(result).expect("AttachListenerResult serializes"))
    });

    let detach_app = Arc::clone(&app);
    server.route("detach_listener", move |params| {
        let typed: DetachListenerParams =
            decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?;
        let result = detach_app
            .detach_listener(typed)
            .map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(result).expect("DetachListenerResult serializes"))
    });

    let list_listeners_app = Arc::clone(&app);
    server.route("list_listeners", move |params| {
        let typed: Option<ListListenersParams> = if params.is_null()
            || (params.is_object() && params.as_object().is_some_and(|o| o.is_empty()))
        {
            None
        } else {
            Some(decode_params(&params).map_err(|e| Error::invalid_params(e.to_string()))?)
        };
        let result = list_listeners_app
            .list_listeners(typed)
            .map_err(|e| Error::invalid_params(e.to_string()))?;
        Ok(serde_json::to_value(result).expect("ListListenersResult serializes"))
    });
}

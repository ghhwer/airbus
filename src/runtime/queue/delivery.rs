//! TCP request/acknowledgment transport, independent of queue dispatch mode.
use super::ListenerRegistration;
use crate::proto::payloads::{
    ListenerEventParams, ListenerEventParamsId, ListenerEventResult, QueueName,
};
use crate::proto::rpc;
use crate::runtime::UuidV7;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub(super) fn deliver_event(
    listener: &ListenerRegistration,
    queue: &str,
    event_id: UuidV7,
    event: &Value,
) -> Result<(), String> {
    let params = ListenerEventParams {
        queue: QueueName::try_from(queue).map_err(|e| e.to_string())?,
        id: ListenerEventParamsId::try_from(event_id.to_string()).map_err(|e| e.to_string())?,
        event: serde_json::from_value(event.clone()).map_err(|e| e.to_string())?,
    };
    let payload = rpc::request_with_params(
        "on_event",
        serde_json::to_value(params).map_err(|e| e.to_string())?,
        Value::from(1),
    );
    let body = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;

    let timeout = Duration::from_secs(2).min(listener.exhaustion_timeout);
    let addr_str = format!("{}:{}", listener.host, listener.port);
    let addr: SocketAddr = match addr_str.parse() {
        Ok(a) => a,
        Err(_) => match addr_str.to_socket_addrs() {
            Ok(mut addrs) => match addrs.next() {
                Some(a) => a,
                None => return Err(format!("cannot resolve address: {addr_str}")),
            },
            Err(e) => return Err(format!("cannot resolve address {addr_str}: {e}")),
        },
    };

    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("connect failed to {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;

    stream.write_all(&body).map_err(|e| e.to_string())?;
    let _ = stream.shutdown(Shutdown::Write);

    let mut resp_bytes = Vec::new();
    stream
        .read_to_end(&mut resp_bytes)
        .map_err(|e| e.to_string())?;

    rpc::decode_response::<ListenerEventResult>(&resp_bytes, &Value::from(1))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn rejects_invalid_acknowledgments_over_tcp() {
        for reply in [
            r#"{"id":1,"result":{"status":"ok"}}"#,
            r#"{"jsonrpc":"1.0","id":1,"result":{"status":"ok"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{"status":"ok"}}"#,
            r#"{"jsonrpc":"2.0","id":1,"result":null}"#,
            r#"{"jsonrpc":"2.0","id":1,"result":{"status":"wrong"}}"#,
        ] {
            let socket = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = socket.local_addr().unwrap().port();
            let peer = std::thread::spawn(move || {
                let (mut stream, _) = socket.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                stream.write_all(reply.as_bytes()).unwrap();
            });
            let listener = ListenerRegistration::new(
                UuidV7::generate(),
                "127.0.0.1".into(),
                port,
                3,
                Duration::from_secs(2),
            );
            let result = deliver_event(
                &listener,
                "demo",
                UuidV7::generate(),
                &serde_json::json!({"n":1}),
            );
            peer.join().unwrap();
            assert!(result.is_err(), "accepted invalid acknowledgment: {reply}");
        }
    }
}

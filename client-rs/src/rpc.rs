//! Async TCP JSON-RPC client and push-event listener.

use crate::endpoint::{airbus_endpoint, parse_endpoint};
use crate::payloads::{
    AddParams, AddResult, AttachListenerParams, AttachListenerResult, CreateQueueParams,
    CreateQueueResult, DeleteQueueParams, DeleteQueueResult, DetachListenerParams,
    DetachListenerParamsListenerId, DetachListenerResult, DuplexSide, ListListenersParams,
    ListListenersResult, ListQueuesResult, PeekEventsParams, PeekEventsResult, PingResult,
    PostEventParams, PostEventResult, QueueReadyParams, QueueReadyResult,
};
use crate::protocol::{self, OnEventDecode, ProtocolError, RpcError};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::future::Future;
use std::num::NonZeroU64;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tokio::time::timeout;

type EventCallback = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync,
>;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error(transparent)]
    Rpc(#[from] RpcError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl From<String> for ClientError {
    fn from(value: String) -> Self {
        ClientError::Other(value)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(value: serde_json::Error) -> Self {
        ClientError::Protocol(ProtocolError::from(value))
    }
}

/// Placeholder filled when a batch is sent; read via [`Pending::result`].
#[derive(Debug)]
pub struct Pending<T> {
    inner: Arc<Mutex<PendingInner<T>>>,
}

#[derive(Debug)]
struct PendingInner<T> {
    done: bool,
    value: Option<T>,
    error: Option<RpcError>,
}

impl<T> Pending<T> {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PendingInner {
                done: false,
                value: None,
                error: None,
            })),
        }
    }

    pub async fn result(&self) -> Result<T, ClientError>
    where
        T: Clone,
    {
        let guard = self.inner.lock().await;
        if !guard.done {
            return Err(ClientError::Other(
                "batch has not been sent yet".into(),
            ));
        }
        if let Some(err) = &guard.error {
            return Err(ClientError::Rpc(err.clone()));
        }
        Ok(guard.value.clone().expect("pending value"))
    }

    async fn resolve_ok(&self, value: T) {
        let mut guard = self.inner.lock().await;
        guard.value = Some(value);
        guard.done = true;
    }

    async fn resolve_err(&self, error: RpcError) {
        let mut guard = self.inner.lock().await;
        guard.error = Some(error);
        guard.done = true;
    }
}

struct BatchSlot {
    pending: Arc<dyn ResolveSlot + Send + Sync>,
}

trait ResolveSlot: Send + Sync {
    fn resolve<'a>(
        &'a self,
        response: Value,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

struct TypedSlot<T> {
    pending: Pending<T>,
}

impl<T> ResolveSlot for TypedSlot<T>
where
    T: DeserializeOwned + Send + Sync + 'static,
{
    fn resolve<'a>(
        &'a self,
        response: Value,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            match protocol::response_result(&response) {
                Ok(raw) => match protocol::decode_result::<T>(raw) {
                    Ok(value) => self.pending.resolve_ok(value).await,
                    Err(err) => {
                        self.pending
                            .resolve_err(RpcError::new(
                                protocol::INTERNAL_ERROR,
                                err.to_string(),
                            ))
                            .await;
                    }
                },
                Err(err) => self.pending.resolve_err(err).await,
            }
        })
    }
}

/// Collects calls/notifies into one JSON-RPC batch document.
pub struct Batch {
    client: RpcClient,
    next_id: i64,
    requests: Vec<Value>,
    slots: Vec<Option<BatchSlot>>,
    sent: bool,
}

impl Batch {
    fn new(client: RpcClient) -> Self {
        Self {
            client,
            next_id: 1,
            requests: Vec::new(),
            slots: Vec::new(),
            sent: false,
        }
    }

    fn ensure_open(&self) -> Result<(), ClientError> {
        if self.sent {
            return Err(ClientError::Other(
                "cannot enqueue on a sent batch".into(),
            ));
        }
        Ok(())
    }

    pub fn enqueue_notify(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), ClientError> {
        self.ensure_open()?;
        self.requests
            .push(protocol::request(method, params, None));
        self.slots.push(None);
        Ok(())
    }

    pub fn enqueue_call<T>(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Pending<T>, ClientError>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        self.ensure_open()?;
        let id = Value::from(self.next_id);
        self.next_id += 1;
        let pending = Pending::new();
        self.requests
            .push(protocol::request(method, params, Some(id)));
        self.slots.push(Some(BatchSlot {
            pending: Arc::new(TypedSlot {
                pending: Pending {
                    inner: Arc::clone(&pending.inner),
                },
            }),
        }));
        Ok(pending)
    }

    pub fn ping(&mut self) -> Result<Pending<PingResult>, ClientError> {
        self.enqueue_call("ping", None)
    }

    pub fn notify_ping(&mut self) -> Result<(), ClientError> {
        self.enqueue_notify("ping", None)
    }

    pub fn create_queue(
        &mut self,
        params: &CreateQueueParams,
    ) -> Result<Pending<CreateQueueResult>, ClientError> {
        let wire = protocol::to_wire(params)?;
        self.enqueue_call("create_queue", Some(wire))
    }

    pub fn post_event(
        &mut self,
        params: &PostEventParams,
    ) -> Result<Pending<PostEventResult>, ClientError> {
        let wire = protocol::to_wire(params)?;
        self.enqueue_call("post_event", Some(wire))
    }

    pub fn peek_events(
        &mut self,
        params: &PeekEventsParams,
    ) -> Result<Pending<PeekEventsResult>, ClientError> {
        let wire = protocol::to_wire(params)?;
        self.enqueue_call("peek_events", Some(wire))
    }

    pub async fn send(mut self) -> Result<(), ClientError> {
        if self.sent {
            return Err(ClientError::Other("batch already sent".into()));
        }
        self.sent = true;
        if self.requests.is_empty() {
            return Ok(());
        }
        let response = self.client.raw(&Value::Array(self.requests.clone())).await?;
        let documents = protocol::batch_responses(&self.requests, response.as_ref())?;
        for (document, slot) in documents.into_iter().zip(self.slots) {
            if let Some(slot) = slot {
                let document = document.expect("batch call must have response");
                slot.pending.resolve(document).await;
            }
        }
        Ok(())
    }
}

/// Client-side port listener that receives real-time events pushed from Airbus.
pub struct EventListener {
    client: RpcClient,
    queue: String,
    host: String,
    exhaustion_timeout_ms: Option<u64>,
    max_retries: Option<u64>,
    max_events: Option<usize>,
    side: Option<DuplexSide>,
    listener: Option<TcpListener>,
    port: u16,
    listener_id: Option<String>,
    task: Option<JoinHandle<()>>,
    stop: Arc<Notify>,
    events: Arc<Mutex<VecDeque<Value>>>,
    on_event: Option<EventCallback>,
}

impl EventListener {
    fn new(client: RpcClient, queue: String) -> Self {
        Self {
            client,
            queue,
            host: "127.0.0.1".into(),
            exhaustion_timeout_ms: None,
            max_retries: None,
            max_events: None,
            side: None,
            listener: None,
            port: 0,
            listener_id: None,
            task: None,
            stop: Arc::new(Notify::new()),
            events: Arc::new(Mutex::new(VecDeque::new())),
            on_event: None,
        }
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn exhaustion_timeout_ms(mut self, ms: u64) -> Self {
        self.exhaustion_timeout_ms = Some(ms);
        self
    }

    pub fn max_retries(mut self, n: u64) -> Self {
        self.max_retries = Some(n);
        self
    }

    pub fn max_events(mut self, n: usize) -> Self {
        self.max_events = Some(n);
        self
    }

    pub fn side(mut self, side: DuplexSide) -> Self {
        self.side = Some(side);
        self
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn listener_id(&self) -> Option<&str> {
        self.listener_id.as_deref()
    }

    pub async fn events(&self) -> Vec<Value> {
        self.events.lock().await.iter().cloned().collect()
    }

    /// Bind, attach, and start accepting `on_event` deliveries.
    pub async fn start<F, Fut>(mut self, on_event: F) -> Result<Self, ClientError>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let listener = TcpListener::bind((self.host.as_str(), 0)).await?;
        self.port = listener.local_addr()?.port();
        self.listener = Some(listener);
        self.on_event = Some(Arc::new(move |event| Box::pin(on_event(event))));

        let port = NonZeroU64::new(u64::from(self.port))
            .ok_or_else(|| ClientError::Other("listener port must be non-zero".into()))?;
        let queue = self
            .queue
            .parse()
            .map_err(|e: crate::payloads::error::ConversionError| ClientError::Other(e.to_string()))?;

        let attach = match self
            .client
            .attach_listener(&AttachListenerParams {
                queue,
                port,
                host: Some(self.host.clone()),
                exhaustion_timeout_ms: self.exhaustion_timeout_ms,
                max_retries: self.max_retries,
                side: self.side,
            })
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.close().await;
                return Err(err);
            }
        };
        self.listener_id = Some(attach.listener_id.to_string());

        let tcp = self.listener.take().expect("listener bound");
        let stop = Arc::clone(&self.stop);
        let events = Arc::clone(&self.events);
        let on_event = Arc::clone(self.on_event.as_ref().unwrap());
        let max_events = self.max_events;
        self.task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.notified() => break,
                    accepted = tcp.accept() => {
                        let Ok((mut stream, _)) = accepted else { continue };
                        let mut buf = Vec::new();
                        if stream.read_to_end(&mut buf).await.is_err() {
                            continue;
                        }
                        let body = String::from_utf8_lossy(&buf);
                        let body = body.trim();
                        if body.is_empty() {
                            continue;
                        }
                        let reply = match protocol::decode_on_event(body) {
                            OnEventDecode::Reply(reply) => reply,
                            OnEventDecode::Event(decoded) => {
                                {
                                    let mut guard = events.lock().await;
                                    if let Some(max) = max_events {
                                        while guard.len() >= max {
                                            guard.pop_front();
                                        }
                                    }
                                    guard.push_back(decoded.event.clone());
                                }
                                let cb_result = on_event(decoded.event).await;
                                decoded.id.map(|id| match cb_result {
                                    Ok(()) => protocol::event_ack(id),
                                    Err(err) => {
                                        protocol::event_error(id, protocol::INTERNAL_ERROR, &err)
                                    }
                                })
                            }
                        };
                        if let Some(response) = reply {
                            let bytes = serde_json::to_vec(&response).unwrap_or_default();
                            let _ = stream.write_all(&bytes).await;
                            let _ = stream.shutdown().await;
                        }
                    }
                }
            }
        }));

        Ok(self)
    }

    pub async fn close(&mut self) {
        self.stop.notify_waiters();
        if let Some(id) = self.listener_id.take() {
            if let Ok(listener_id) = id.parse::<DetachListenerParamsListenerId>() {
                let _ = self
                    .client
                    .detach_listener(&DetachListenerParams { listener_id })
                    .await;
            }
        }
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
        self.listener = None;
    }
}

impl Drop for EventListener {
    fn drop(&mut self) {
        self.stop.notify_waiters();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub struct RpcClient {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
}

impl RpcClient {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            timeout: Duration::from_secs(2),
        }
    }

    pub fn from_env() -> Result<Self, ClientError> {
        let (host, port) = airbus_endpoint().map_err(ClientError::Other)?;
        Ok(Self::new(host, port))
    }

    pub fn from_url(url: &str) -> Result<Self, ClientError> {
        let (host, port) = parse_endpoint(url).map_err(ClientError::Other)?;
        Ok(Self::new(host, port))
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn batch(&self) -> Batch {
        Batch::new(self.clone())
    }

    pub async fn raw(&self, payload: &Value) -> Result<Option<Value>, ClientError> {
        self.raw_text(&payload.to_string()).await
    }

    pub async fn raw_text(&self, text: &str) -> Result<Option<Value>, ClientError> {
        let connect = TcpStream::connect((self.host.as_str(), self.port));
        let mut stream = timeout(self.timeout, connect)
            .await
            .map_err(|_| ClientError::Other("connect timed out".into()))??;
        let buf = timeout(self.timeout, async {
            stream.write_all(text.as_bytes()).await?;
            stream.shutdown().await?;
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await?;
            Ok::<_, ClientError>(buf)
        })
        .await
        .map_err(|_| ClientError::Other("rpc timed out".into()))??;

        let body = String::from_utf8_lossy(&buf);
        let body = body.trim();
        if body.is_empty() {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_str(body)?))
        }
    }

    pub async fn call(
        &self,
        method: &str,
        params: Option<Value>,
        id: Value,
    ) -> Result<Value, ClientError> {
        let request = protocol::request(method, params, Some(id.clone()));
        let response = self
            .raw(&request)
            .await?
            .ok_or_else(|| ClientError::Other("empty response".into()))?;
        Ok(protocol::validate_response(&response, &id)?)
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), ClientError> {
        let request = protocol::request(method, params, None);
        if self.raw(&request).await?.is_some() {
            return Err(ClientError::Other(
                "JSON-RPC notification must not produce a response".into(),
            ));
        }
        Ok(())
    }

    async fn invoke<P, R>(&self, method: &str, params: Option<&P>) -> Result<R, ClientError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let wire = match params {
            Some(p) => Some(protocol::to_wire(p)?),
            None => None,
        };
        let response = self.call(method, wire, Value::from(1)).await?;
        let result = protocol::response_result(&response)?;
        Ok(protocol::decode_result(result)?)
    }

    pub async fn ping(&self) -> Result<PingResult, ClientError> {
        self.invoke::<Value, _>("ping", None).await
    }

    pub async fn add(&self, params: AddParams) -> Result<AddResult, ClientError> {
        self.invoke("add", Some(&params)).await
    }

    pub async fn create_queue(
        &self,
        params: &CreateQueueParams,
    ) -> Result<CreateQueueResult, ClientError> {
        self.invoke("create_queue", Some(params)).await
    }

    pub async fn delete_queue(
        &self,
        params: &DeleteQueueParams,
    ) -> Result<DeleteQueueResult, ClientError> {
        self.invoke("delete_queue", Some(params)).await
    }

    pub async fn post_event(
        &self,
        params: &PostEventParams,
    ) -> Result<PostEventResult, ClientError> {
        self.invoke("post_event", Some(params)).await
    }

    pub async fn list_queues(&self) -> Result<ListQueuesResult, ClientError> {
        self.invoke::<Value, _>("list_queues", None).await
    }

    pub async fn peek_events(
        &self,
        params: &PeekEventsParams,
    ) -> Result<PeekEventsResult, ClientError> {
        self.invoke("peek_events", Some(params)).await
    }

    pub async fn attach_listener(
        &self,
        params: &AttachListenerParams,
    ) -> Result<AttachListenerResult, ClientError> {
        self.invoke("attach_listener", Some(params)).await
    }

    pub async fn detach_listener(
        &self,
        params: &DetachListenerParams,
    ) -> Result<DetachListenerResult, ClientError> {
        self.invoke("detach_listener", Some(params)).await
    }

    pub async fn list_listeners(
        &self,
        params: Option<&ListListenersParams>,
    ) -> Result<ListListenersResult, ClientError> {
        match params {
            Some(p) => self.invoke("list_listeners", Some(p)).await,
            None => self.invoke::<Value, _>("list_listeners", None).await,
        }
    }

    pub async fn queue_ready(
        &self,
        params: &QueueReadyParams,
    ) -> Result<QueueReadyResult, ClientError> {
        self.invoke("queue_ready", Some(params)).await
    }

    pub fn listen(&self, queue: impl Into<String>) -> EventListener {
        EventListener::new(self.clone(), queue.into())
    }
}

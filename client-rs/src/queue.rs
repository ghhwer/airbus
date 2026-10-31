//! Thin named-queue handle — ensure / delete / attach / post / readiness over Airbus RPC.
//!
//! [`Queue::connect`] / [`Queue::connect_with`] **ensure** the named queue exists with
//! the given [`QueueMode`] (and optional [`DispatchStrategy`]): create if missing, or
//! error if it already exists with different config. That is a network round-trip.
//!
//! Listen host defaults to auto-discovery toward the Airbus peer (see
//! [`crate::endpoint::resolve_listen_host`]); pass [`Queue::with_listen_host`] or set
//! `AIRBUS_LISTEN_HOST` to override.

use crate::payloads::{
    CreateQueueParams, DeleteQueueParams, DispatchStrategy, DuplexSide, EventObject,
    PostEventParams, QueueMode, QueueName, QueueReadyParams,
};
use crate::protocol::RpcError;
use crate::rpc::{ClientError, EventListener, RpcClient};
use serde_json::{Map, Value};
use std::future::Future;

/// Named queue handle for any [`QueueMode`] (optional sticky duplex `side`).
pub struct Queue {
    client: RpcClient,
    pub name: String,
    pub mode: QueueMode,
    pub side: Option<DuplexSide>,
    pub dispatch_strategy: Option<DispatchStrategy>,
    listen_host: Option<String>,
    listener: Option<EventListener>,
}

impl Queue {
    /// Ensure the queue exists with `mode` (strategy defaults per daemon rules).
    pub async fn connect(
        name: impl Into<String>,
        mode: QueueMode,
        client: Option<RpcClient>,
    ) -> Result<Self, ClientError> {
        Self::connect_with(name, mode, None, client).await
    }

    /// Ensure the queue exists with `mode` and optional `dispatch_strategy`.
    pub async fn connect_with(
        name: impl Into<String>,
        mode: QueueMode,
        dispatch_strategy: Option<DispatchStrategy>,
        client: Option<RpcClient>,
    ) -> Result<Self, ClientError> {
        let client = match client {
            Some(c) => c,
            None => RpcClient::from_env()?,
        };
        let q = Self {
            client,
            name: name.into(),
            mode,
            side: None,
            dispatch_strategy,
            listen_host: None,
            listener: None,
        };
        q.ensure().await?;
        Ok(q)
    }

    async fn ensure(&self) -> Result<(), ClientError> {
        let queue: QueueName = self
            .name
            .parse()
            .map_err(|e: crate::payloads::error::ConversionError| ClientError::Other(e.to_string()))?;
        self.client
            .create_queue(&CreateQueueParams {
                queue,
                mode: Some(self.mode),
                dispatch_strategy: self.dispatch_strategy,
            })
            .await?;
        Ok(())
    }

    pub fn with_side(mut self, side: DuplexSide) -> Self {
        self.side = Some(side);
        self
    }

    /// Pin the callback host registered with Airbus (skips auto-discovery).
    pub fn with_listen_host(mut self, host: impl Into<String>) -> Self {
        self.listen_host = Some(host.into());
        self
    }

    pub async fn delete(&mut self) -> Result<bool, ClientError> {
        self.close().await;
        let queue: QueueName = self
            .name
            .parse()
            .map_err(|e: crate::payloads::error::ConversionError| ClientError::Other(e.to_string()))?;
        let result = self
            .client
            .delete_queue(&DeleteQueueParams { queue })
            .await?;
        Ok(result.deleted)
    }

    pub async fn attach<F, Fut>(
        &mut self,
        on_event: F,
        max_events: Option<usize>,
        exhaustion_timeout_ms: Option<u64>,
        max_retries: Option<u64>,
    ) -> Result<&EventListener, ClientError>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        if self.listener.is_some() {
            self.close().await;
        }
        let mut listener = EventListener::new(self.client.clone(), self.name.clone());
        if let Some(host) = self.listen_host.clone() {
            listener = listener.host(host);
        }
        if let Some(side) = self.side {
            listener = listener.side(side);
        }
        if let Some(n) = max_events {
            listener = listener.max_events(n);
        } else {
            listener = listener.max_events(100);
        }
        if let Some(ms) = exhaustion_timeout_ms {
            listener = listener.exhaustion_timeout_ms(ms);
        }
        if let Some(n) = max_retries {
            listener = listener.max_retries(n);
        }
        let listener = listener.start(on_event).await?;
        self.listener = Some(listener);
        Ok(self.listener.as_ref().unwrap())
    }

    pub async fn try_attach<F, Fut>(
        &mut self,
        on_event: F,
        max_events: Option<usize>,
        exhaustion_timeout_ms: Option<u64>,
        max_retries: Option<u64>,
    ) -> Result<Option<&EventListener>, ClientError>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        match self
            .attach(on_event, max_events, exhaustion_timeout_ms, max_retries)
            .await
        {
            Ok(listener) => Ok(Some(listener)),
            Err(ClientError::Rpc(RpcError { code, message, .. }))
                if code == protocol_invalid_params() && message.contains("does not exist") =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    pub async fn post(&self, event: Map<String, Value>) -> Result<bool, ClientError> {
        let queue: QueueName = self
            .name
            .parse()
            .map_err(|e: crate::payloads::error::ConversionError| ClientError::Other(e.to_string()))?;
        match self
            .client
            .post_event(&PostEventParams {
                queue,
                event: EventObject(event),
                side: self.side,
            })
            .await
        {
            Ok(_) => Ok(true),
            Err(ClientError::Rpc(RpcError { code, message, .. }))
                if code == protocol_invalid_params() && message.contains("does not exist") =>
            {
                Ok(false)
            }
            Err(err) => Err(err),
        }
    }

    pub async fn is_ready(&self) -> Result<bool, ClientError> {
        let queue: QueueName = self
            .name
            .parse()
            .map_err(|e: crate::payloads::error::ConversionError| ClientError::Other(e.to_string()))?;
        let result = self
            .client
            .queue_ready(&QueueReadyParams { queue })
            .await?;
        Ok(result.ready)
    }

    pub async fn close(&mut self) {
        if let Some(mut listener) = self.listener.take() {
            listener.close().await;
        }
    }
}

fn protocol_invalid_params() -> i64 {
    crate::protocol::INVALID_PARAMS
}

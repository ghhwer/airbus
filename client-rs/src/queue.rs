//! Thin named-queue handle — create / delete / attach / post / readiness over Airbus RPC.

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
    pub side: Option<DuplexSide>,
    listen_host: String,
    listener: Option<EventListener>,
}

impl Queue {
    pub fn new(name: impl Into<String>, client: Option<RpcClient>) -> Result<Self, ClientError> {
        let client = match client {
            Some(c) => c,
            None => RpcClient::from_env()?,
        };
        Ok(Self {
            client,
            name: name.into(),
            side: None,
            listen_host: "127.0.0.1".into(),
            listener: None,
        })
    }

    pub fn with_side(mut self, side: DuplexSide) -> Self {
        self.side = Some(side);
        self
    }

    pub fn with_listen_host(mut self, host: impl Into<String>) -> Self {
        self.listen_host = host.into();
        self
    }

    pub async fn create(
        &self,
        mode: QueueMode,
        dispatch_strategy: Option<DispatchStrategy>,
    ) -> Result<bool, ClientError> {
        let queue: QueueName = self
            .name
            .parse()
            .map_err(|e: crate::payloads::error::ConversionError| ClientError::Other(e.to_string()))?;
        let result = self
            .client
            .create_queue(&CreateQueueParams {
                queue,
                mode: Some(mode),
                dispatch_strategy,
            })
            .await?;
        Ok(result.created)
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
        let mut listener = self.client.listen(self.name.clone()).host(self.listen_host.clone());
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

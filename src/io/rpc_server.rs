use crate::proto::rpc;

/// Byte-server adapter: request bytes → JSON-RPC document, then bytes out.
pub struct RpcServer {
    router: rpc::Server,
}

impl RpcServer {
    pub fn new() -> Self {
        Self {
            router: rpc::Server::new(),
        }
    }

    pub fn route<F>(&mut self, method: impl Into<String>, handler: F)
    where
        F: Fn(serde_json::Value) -> Result<serde_json::Value, rpc::Error> + Send + Sync + 'static,
    {
        self.router.add(method, handler);
    }

    pub fn on_bytes(&self, request: &[u8]) -> String {
        let text = String::from_utf8_lossy(request);
        self.router.handle(&text)
    }

    pub fn serve_tcp(&self, host: &str, port: u16) -> Result<(), String> {
        super::server::serve_tcp(host, port, |bytes| self.on_bytes(bytes))
    }
}

impl Default for RpcServer {
    fn default() -> Self {
        Self::new()
    }
}

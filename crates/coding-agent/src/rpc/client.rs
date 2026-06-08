use std::collections::BTreeMap;
use thiserror::Error;

use crate::rpc::jsonl::serialize_json_line;
use crate::rpc::types::{RpcCommand, RpcEvent, RpcResponse};

pub trait RpcTransport {
    fn send_line(&mut self, line: &str) -> Result<(), RpcClientError>;
}

#[derive(Debug, Error)]
pub enum RpcClientError {
    #[error("RPC transport failed: {0}")]
    Transport(String),
    #[error("RPC response missing for request {0}")]
    MissingResponse(String),
    #[error("RPC command failed: {0}")]
    Command(String),
    #[error("RPC JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct RpcClient<T: RpcTransport> {
    transport: T,
    request_id: u64,
    pending_responses: BTreeMap<String, RpcResponse>,
    events: Vec<RpcEvent>,
}

impl<T: RpcTransport> RpcClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            request_id: 0,
            pending_responses: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    pub fn send(&mut self, command: RpcCommand) -> Result<String, RpcClientError> {
        self.request_id += 1;
        let id = format!("req_{}", self.request_id);
        let command = command.with_id(id.clone());
        let line = serialize_json_line(&command)?;
        self.transport.send_line(&line)?;
        Ok(id)
    }

    pub fn receive_response(&mut self, response: RpcResponse) {
        if let Some(id) = response.id() {
            self.pending_responses.insert(id.to_string(), response);
        }
    }

    pub fn receive_event(&mut self, event: RpcEvent) {
        self.events.push(event);
    }

    pub fn take_response(&mut self, id: &str) -> Result<RpcResponse, RpcClientError> {
        let response = self
            .pending_responses
            .remove(id)
            .ok_or_else(|| RpcClientError::MissingResponse(id.to_string()))?;
        if !response.is_success() {
            let error = match &response {
                RpcResponse::Response { error, .. } => {
                    error.clone().unwrap_or_else(|| "unknown error".to_string())
                }
            };
            return Err(RpcClientError::Command(error));
        }
        Ok(response)
    }

    pub fn drain_events(&mut self) -> Vec<RpcEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn into_transport(self) -> T {
        self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryTransport {
        lines: Vec<String>,
    }

    impl RpcTransport for MemoryTransport {
        fn send_line(&mut self, line: &str) -> Result<(), RpcClientError> {
            self.lines.push(line.to_string());
            Ok(())
        }
    }

    #[test]
    fn sends_command_with_request_id() {
        let mut client = RpcClient::new(MemoryTransport::default());
        let id = client
            .send(RpcCommand::GetState { id: None })
            .expect("send should work");
        let transport = client.into_transport();

        assert_eq!(id, "req_1");
        assert_eq!(transport.lines.len(), 1);
        assert!(transport.lines[0].contains("\"id\":\"req_1\""));
        assert!(transport.lines[0].ends_with('\n'));
    }

    #[test]
    fn stores_and_returns_matching_response() {
        let mut client = RpcClient::new(MemoryTransport::default());
        client.receive_response(RpcResponse::ok(
            Some("req_1".to_string()),
            "get_state",
            Some(serde_json::json!({"sessionId":"s"})),
        ));

        let response = client.take_response("req_1").expect("response");
        assert!(response.is_success());
    }
}

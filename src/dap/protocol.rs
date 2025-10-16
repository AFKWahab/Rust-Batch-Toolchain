use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct DapMessage {
    pub seq: u64,
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(flatten)]
    pub content: DapMessageContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DapMessageContent {
    Request {
        command: String,
        arguments: Option<Value>,
    },
    Response {
        request_seq: u64,
        success: bool,
        command: String,
        message: Option<String>,
        body: Option<Value>,
    },
    Event {
        event: String,
        body: Option<Value>,
    },
}

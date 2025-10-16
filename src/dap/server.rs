use super::protocol::{DapMessage, DapMessageContent};
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};

pub struct DapServer {
    seq: u64,
}

impl DapServer {
    pub fn new() -> Self {
        Self { seq: 0 }
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    pub fn send_response(
        &mut self,
        request_seq: u64,
        command: String,
        success: bool,
        body: Option<Value>,
    ) {
        let msg = DapMessage {
            seq: self.next_seq(),
            msg_type: "response".to_string(),
            content: DapMessageContent::Response {
                request_seq,
                success,
                command,
                message: None,
                body,
            },
        };
        self.send_message(&msg);
    }

    pub fn send_event(&mut self, event: String, body: Option<Value>) {
        let msg = DapMessage {
            seq: self.next_seq(),
            msg_type: "event".to_string(),
            content: DapMessageContent::Event { event, body },
        };
        self.send_message(&msg);
    }

    fn send_message(&self, msg: &DapMessage) {
        let json = serde_json::to_string(msg).unwrap();
        let content_length = json.len();
        // Use stdout for DAP protocol, stderr for debug messages
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        let _ = writeln!(handle, "Content-Length: {}\r\n", content_length);
        let _ = writeln!(handle, "{}", json);
        let _ = handle.flush();
    }

    pub fn read_message(&self) -> Option<DapMessage> {
        let stdin = io::stdin();
        let mut handle = stdin.lock();

        // Read Content-Length header
        let mut content_length = 0;
        let mut lines = handle.by_ref().lines();

        loop {
            if let Some(Ok(line)) = lines.next() {
                if line.is_empty() || line == "\r" {
                    break;
                }
                if line.starts_with("Content-Length:") {
                    content_length = line[15..].trim().parse().unwrap_or(0);
                }
            } else {
                return None;
            }
        }

        // Read JSON body
        if content_length > 0 {
            let mut buffer = vec![0u8; content_length];
            drop(lines); // Drop lines iterator to release the lock
            if handle.read_exact(&mut buffer).is_ok() {
                if let Ok(msg) = serde_json::from_slice(&buffer) {
                    return Some(msg);
                }
            }
        }

        None
    }

    pub fn handle_initialize(&mut self, seq: u64, command: String) {
        let body = json!({
            "supportsConfigurationDoneRequest": true,
            "supportsStepBack": false,
            "supportsStepInTargetsRequest": false,
        });
        self.send_response(seq, command, true, Some(body));
    }

    pub fn handle_launch(&mut self, seq: u64, command: String) {
        self.send_response(seq, command, true, None);
        self.send_event(
            "stopped".to_string(),
            Some(json!({
                "reason": "entry",
                "threadId": 1
            })),
        );
    }

    pub fn handle_set_breakpoints(&mut self, seq: u64, command: String) {
        self.send_response(
            seq,
            command,
            true,
            Some(json!({
                "breakpoints": []
            })),
        );
    }

    pub fn handle_step(&mut self, seq: u64, command: String) {
        self.send_response(seq, command, true, None);
    }
}

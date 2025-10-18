use super::protocol::{DapMessage, DapMessageContent};
use crate::debugger::{CmdSession, DebugContext, RunMode};
use crate::executor;
use crate::parser::{self, PreprocessResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct DapServer {
    seq: u64,
    context: Option<Arc<Mutex<DebugContext>>>,
    preprocessed: Option<PreprocessResult>,
    labels: Option<HashMap<String, usize>>,
    breakpoints: HashMap<String, Vec<usize>>,
    event_sender: Option<Sender<DapEvent>>,
    program_path: Option<String>, // ADDED
}

// Events that the execution thread can send back
#[derive(Debug)]
enum DapEvent {
    Stopped { reason: String, line: usize },
    Exited { exit_code: i32 },
}

impl DapServer {
    pub fn new() -> Self {
        Self {
            seq: 0,
            context: None,
            preprocessed: None,
            labels: None,
            breakpoints: HashMap::new(),
            event_sender: None,
            program_path: None, // ADDED
        }
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

        // Write to stdout with proper protocol format
        // CRITICAL: Must be exactly "Content-Length: {len}\r\n\r\n{json}"
        let output = format!("Content-Length: {}\r\n\r\n{}", content_length, json);

        print!("{}", output);

        // MUST flush immediately
        use std::io::Write;
        let _ = std::io::stdout().flush();

        // Debug log to stderr (won't interfere with DAP protocol)
        eprintln!("📤 Sent {} bytes", content_length);
    }

    pub fn read_message(&self) -> Option<DapMessage> {
        let stdin = io::stdin();
        let mut handle = stdin.lock();

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

        if content_length > 0 {
            let mut buffer = vec![0u8; content_length];
            drop(lines);
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
            "supportsFunctionBreakpoints": false,
            "supportsConditionalBreakpoints": false,
            "supportsSetVariable": false,
        });
        self.send_response(seq, command, true, Some(body));

        // CRITICAL: Send initialized event after response
        eprintln!("📋 Sending initialized event");
        self.send_event("initialized".to_string(), None);
    }

    pub fn handle_launch(&mut self, seq: u64, command: String, args: Option<Value>) {
        let program = args
            .as_ref()
            .and_then(|v| v.get("program"))
            .and_then(|v| v.as_str())
            .unwrap_or("test.bat");

        // STORE the program path
        self.program_path = Some(program.to_string());

        eprintln!("🚀 Launching batch file: {}", program);

        match std::fs::read_to_string(program) {
            Ok(contents) => {
                let physical_lines: Vec<&str> = contents.lines().collect();
                let pre = parser::preprocess_lines(&physical_lines);
                let labels_phys = parser::build_label_map(&physical_lines);

                match CmdSession::start() {
                    Ok(session) => {
                        let mut ctx = DebugContext::new(session);
                        ctx.set_mode(RunMode::StepInto);

                        let ctx_arc = Arc::new(Mutex::new(ctx));
                        self.context = Some(ctx_arc.clone());
                        self.preprocessed = Some(pre.clone());
                        self.labels = Some(labels_phys.clone());

                        let (tx, rx) = channel();
                        self.event_sender = Some(tx.clone());

                        self.send_response(seq, command, true, None);

                        thread::spawn(move || {
                            if let Err(e) = executor::run_debugger_dap(ctx_arc, &pre, &labels_phys)
                            {
                                eprintln!("❌ Execution error: {}", e);
                            }
                            let _ = tx.send(DapEvent::Exited { exit_code: 0 });
                        });

                        thread::sleep(Duration::from_millis(100));
                        self.send_event(
                            "stopped".to_string(),
                            Some(json!({
                                "reason": "entry",
                                "threadId": 1,
                                "allThreadsStopped": true
                            })),
                        );

                        thread::spawn(move || loop {
                            match rx.recv() {
                                Ok(DapEvent::Stopped { reason, line }) => {
                                    eprintln!("📥 Received stopped event: {} at {}", reason, line);
                                }
                                Ok(DapEvent::Exited { exit_code }) => {
                                    eprintln!("📥 Received exit event: {}", exit_code);
                                    break;
                                }
                                Err(_) => break,
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to start CMD session: {}", e);
                        self.send_response(seq, command, false, None);
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to read batch file: {}", e);
                self.send_response(seq, command, false, None);
            }
        }
    }

    pub fn handle_set_breakpoints(&mut self, seq: u64, command: String, args: Option<Value>) {
        let source_path = args
            .as_ref()
            .and_then(|v| v.get("source"))
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let breakpoints_array = args
            .as_ref()
            .and_then(|v| v.get("breakpoints"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut verified_breakpoints = Vec::new();
        let mut logical_lines = Vec::new();

        if let Some(pre) = &self.preprocessed {
            for bp in breakpoints_array {
                if let Some(line) = bp.get("line").and_then(|v| v.as_u64()) {
                    let phys_line = (line as usize).saturating_sub(1);

                    if phys_line < pre.phys_to_logical.len() {
                        let logical_line = pre.phys_to_logical[phys_line];
                        logical_lines.push(logical_line);

                        verified_breakpoints.push(json!({
                            "verified": true,
                            "line": line
                        }));
                    }
                }
            }
        }

        self.breakpoints
            .insert(source_path.to_string(), logical_lines.clone());

        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                for logical_line in logical_lines {
                    ctx.add_breakpoint(logical_line);
                }
            }
        }

        self.send_response(
            seq,
            command,
            true,
            Some(json!({
                "breakpoints": verified_breakpoints
            })),
        );
    }

    pub fn handle_threads(&mut self, seq: u64, command: String) {
        self.send_response(
            seq,
            command,
            true,
            Some(json!({
                "threads": [
                    {
                        "id": 1,
                        "name": "Batch Script"
                    }
                ]
            })),
        );
    }

    pub fn handle_stack_trace(&mut self, seq: u64, command: String) {
        let mut frames = Vec::new();

        // Get the program path (use stored path or fallback)
        let program_path = self.program_path.as_deref().unwrap_or("test.bat");
        let program_name = std::path::Path::new(program_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("test.bat");

        if let Some(ctx_arc) = &self.context {
            if let Ok(ctx) = ctx_arc.lock() {
                if let Some(pre) = &self.preprocessed {
                    // Add current frame
                    frames.push(json!({
                        "id": 0,
                        "name": "main",
                        "line": 1,
                        "column": 1,
                        "source": {
                            "name": program_name,
                            "path": program_path
                        }
                    }));

                    // Add call stack frames
                    for (i, frame) in ctx.call_stack.iter().enumerate() {
                        let return_line = frame.return_pc.saturating_sub(1);
                        if return_line < pre.logical.len() {
                            let logical = &pre.logical[return_line];
                            frames.push(json!({
                                "id": i + 1,
                                "name": format!("frame_{}", i + 1),
                                "line": logical.phys_start + 1,
                                "column": 1,
                                "source": {
                                    "name": program_name,
                                    "path": program_path
                                }
                            }));
                        }
                    }
                }
            }
        }

        self.send_response(
            seq,
            command,
            true,
            Some(json!({
                "stackFrames": frames,
                "totalFrames": frames.len()
            })),
        );
    }

    pub fn handle_scopes(&mut self, seq: u64, command: String) {
        self.send_response(
            seq,
            command,
            true,
            Some(json!({
                "scopes": [
                    {
                        "name": "Local",
                        "variablesReference": 1,
                        "expensive": false
                    },
                    {
                        "name": "Global",
                        "variablesReference": 2,
                        "expensive": false
                    }
                ]
            })),
        );
    }

    pub fn handle_variables(&mut self, seq: u64, command: String, args: Option<Value>) {
        let var_ref = args
            .as_ref()
            .and_then(|v| v.get("variablesReference"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let mut variables = Vec::new();

        if let Some(ctx_arc) = &self.context {
            if let Ok(ctx) = ctx_arc.lock() {
                match var_ref {
                    1 => {
                        let visible = ctx.get_visible_variables();
                        for (key, val) in visible {
                            variables.push(json!({
                                "name": key,
                                "value": val,
                                "variablesReference": 0
                            }));
                        }
                    }
                    2 => {
                        for (key, val) in &ctx.variables {
                            variables.push(json!({
                                "name": key,
                                "value": val,
                                "variablesReference": 0
                            }));
                        }
                    }
                    _ => {}
                }
            }
        }

        self.send_response(
            seq,
            command,
            true,
            Some(json!({
                "variables": variables
            })),
        );
    }

    pub fn handle_continue(&mut self, seq: u64, command: String) {
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::Continue);
                ctx.continue_requested = true;
            }
        }
        self.send_response(
            seq,
            command,
            true,
            Some(json!({"allThreadsContinued": true})),
        );
    }

    pub fn handle_next(&mut self, seq: u64, command: String) {
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::StepOver);
                ctx.continue_requested = true;
            }
        }
        self.send_response(seq, command, true, None);
    }

    pub fn handle_step_in(&mut self, seq: u64, command: String) {
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::StepInto);
                ctx.continue_requested = true;
            }
        }
        self.send_response(seq, command, true, None);
    }

    pub fn handle_step_out(&mut self, seq: u64, command: String) {
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::StepOut);
                ctx.continue_requested = true;
            }
        }
        self.send_response(seq, command, true, None);
    }
}

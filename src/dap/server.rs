use super::protocol::{DapMessage, DapMessageContent};
use crate::debugger::{CmdSession, DebugContext, RunMode};
use crate::executor;
use crate::parser::{self, PreprocessResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender}; // ← Updated this line
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct DapServer {
    seq: u64,
    context: Option<Arc<Mutex<DebugContext>>>,
    preprocessed: Option<PreprocessResult>,
    labels: Option<HashMap<String, usize>>,
    breakpoints: HashMap<String, Vec<usize>>,
    program_path: Option<String>,
    event_receiver: Option<Receiver<(String, usize)>>, // Add this back!
}

impl DapServer {
    pub fn new() -> Self {
        Self {
            seq: 0,
            context: None,
            preprocessed: None,
            labels: None,
            breakpoints: HashMap::new(),
            program_path: None,
            event_receiver: None, // Add this back!
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

        let stop_on_entry = args
            .as_ref()
            .and_then(|v| v.get("stopOnEntry"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        self.program_path = Some(program.to_string());

        eprintln!("🚀 Launching batch file: {}", program);
        eprintln!("   Stop on entry: {}", stop_on_entry);

        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("C:\\temp\\batch-debugger-vscode.log")
            .ok();

        if let Some(ref mut f) = log {
            use std::io::Write;
            writeln!(f, "handle_launch called for: {}", program).ok();
            writeln!(f, "stop_on_entry: {}", stop_on_entry).ok();
            f.flush().ok();
        }

        match std::fs::read_to_string(program) {
            Ok(contents) => {
                let physical_lines: Vec<&str> = contents.lines().collect();
                let pre = parser::preprocess_lines(&physical_lines);
                let labels_phys = parser::build_label_map(&physical_lines);

                eprintln!("📝 Parsed {} logical lines", pre.logical.len());
                if let Some(ref mut f) = log {
                    use std::io::Write;
                    writeln!(f, "Parsed {} logical lines", pre.logical.len()).ok();
                    f.flush().ok();
                }

                match CmdSession::start() {
                    Ok(session) => {
                        eprintln!("✓ CMD session started");
                        if let Some(ref mut f) = log {
                            use std::io::Write;
                            writeln!(f, "CMD session started successfully").ok();
                            f.flush().ok();
                        }

                        let mut ctx = DebugContext::new(session);

                        if stop_on_entry {
                            ctx.set_mode(RunMode::StepInto);
                            eprintln!("   Mode: StepInto (will stop at first line)");
                        } else {
                            ctx.set_mode(RunMode::Continue);
                            eprintln!("   Mode: Continue (will run until breakpoint)");
                        }
                        ctx.continue_requested = false;

                        let ctx_arc = Arc::new(Mutex::new(ctx));
                        self.context = Some(ctx_arc.clone());
                        self.preprocessed = Some(pre.clone());
                        self.labels = Some(labels_phys.clone());

                        self.send_response(seq, command, true, None);
                        eprintln!("📤 Sent launch response");

                        let mut thread_log = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open("C:\\temp\\batch-debugger-vscode.log")
                            .ok();

                        if let Some(ref mut f) = thread_log {
                            use std::io::Write;
                            writeln!(f, "About to spawn execution thread").ok();
                            f.flush().ok();
                        }

                        let (tx, rx) = channel::<(String, usize)>();

                        // Store the receiver so we can use it later
                        self.event_receiver = Some(rx);

                        let exec_ctx = ctx_arc.clone();
                        let exec_pre = pre.clone();
                        let exec_labels = labels_phys.clone();

                        thread::spawn(move || {
                            let mut tlog = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open("C:\\temp\\batch-debugger-vscode.log")
                                .ok();

                            if let Some(ref mut f) = tlog {
                                use std::io::Write;
                                writeln!(f, "🧵 Execution thread STARTED").ok();
                                f.flush().ok();
                            }

                            eprintln!("🧵 Execution thread started");

                            match executor::run_debugger_dap(exec_ctx, &exec_pre, &exec_labels, tx)
                            {
                                Ok(_) => {
                                    eprintln!("✅ Execution completed successfully");
                                    if let Some(ref mut f) = tlog {
                                        use std::io::Write;
                                        writeln!(f, "✅ Execution completed successfully").ok();
                                        f.flush().ok();
                                    }
                                }
                                Err(e) => {
                                    eprintln!("❌ Execution error: {}", e);
                                    if let Some(ref mut f) = tlog {
                                        use std::io::Write;
                                        writeln!(f, "❌ Execution error: {}", e).ok();
                                        f.flush().ok();
                                    }
                                }
                            }

                            if let Some(ref mut f) = tlog {
                                use std::io::Write;
                                writeln!(f, "🧵 Execution thread EXITING").ok();
                                f.flush().ok();
                            }
                            eprintln!("🧵 Execution thread exiting");
                        });

                        if let Some(ref mut f) = log {
                            use std::io::Write;
                            writeln!(f, "Execution thread spawned, waiting for first stop").ok();
                            f.flush().ok();
                        }

                        // Wait for the first stopped event and send it
                        if let Some(ref rx) = self.event_receiver {
                            if let Ok((reason, line)) = rx.recv_timeout(Duration::from_secs(2)) {
                                if let Some(ref mut f) = log {
                                    use std::io::Write;
                                    writeln!(f, "Received first stop: {} at line {}", reason, line)
                                        .ok();
                                    f.flush().ok();
                                }

                                if reason != "terminated" {
                                    self.send_event(
                                        "stopped".to_string(),
                                        Some(json!({
                                            "reason": reason,
                                            "threadId": 1,
                                            "allThreadsStopped": true
                                        })),
                                    );
                                    eprintln!("📤 Sent initial stopped event: {}", reason);
                                } else {
                                    eprintln!("⚠️ Script completed before first stop");
                                    self.send_event("terminated".to_string(), None);
                                }
                            } else {
                                if let Some(ref mut f) = log {
                                    use std::io::Write;
                                    writeln!(f, "⚠️ Timeout waiting for first stop event").ok();
                                    f.flush().ok();
                                }
                                eprintln!("⚠️ Timeout waiting for first stop event");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to start CMD session: {}", e);
                        if let Some(ref mut f) = log {
                            use std::io::Write;
                            writeln!(f, "❌ Failed to start CMD session: {}", e).ok();
                            f.flush().ok();
                        }
                        self.send_response(seq, command, false, None);
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to read batch file: {}", e);
                if let Some(ref mut f) = log {
                    use std::io::Write;
                    writeln!(f, "❌ Failed to read batch file: {}", e).ok();
                    f.flush().ok();
                }
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

        eprintln!("🔍 Setting breakpoints for: {}", source_path);

        if let Some(pre) = &self.preprocessed {
            for bp in breakpoints_array {
                if let Some(line) = bp.get("line").and_then(|v| v.as_u64()) {
                    // VS Code sends 1-based line numbers
                    let phys_line = (line as usize).saturating_sub(1);

                    eprintln!(
                        "   Breakpoint request: physical line {} (0-indexed: {})",
                        line, phys_line
                    );

                    if phys_line < pre.phys_to_logical.len() {
                        let logical_line = pre.phys_to_logical[phys_line];
                        logical_lines.push(logical_line);

                        eprintln!("   ✓ Mapped to logical line {}", logical_line);
                        eprintln!("   Line content: {}", pre.logical[logical_line].text);

                        verified_breakpoints.push(json!({
                            "verified": true,
                            "line": line
                        }));
                    } else {
                        eprintln!("   ✗ Physical line {} out of range", phys_line);
                    }
                }
            }
        }

        self.breakpoints
            .insert(source_path.to_string(), logical_lines.clone());

        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                eprintln!("   Adding {} breakpoints to context", logical_lines.len());
                for logical_line in &logical_lines {
                    ctx.add_breakpoint(*logical_line);
                    eprintln!("   Added breakpoint at logical line {}", logical_line);
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

        let program_path = self.program_path.as_deref().unwrap_or("test.bat");
        let program_name = std::path::Path::new(program_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("test.bat");

        if let Some(ctx_arc) = &self.context {
            if let Ok(ctx) = ctx_arc.lock() {
                if let Some(pre) = &self.preprocessed {
                    // Get the current PC (program counter) from the context
                    let current_pc = ctx.current_line.unwrap_or(0);

                    // Get the physical line number for the current logical line
                    let physical_line = if current_pc < pre.logical.len() {
                        pre.logical[current_pc].phys_start + 1 // +1 because VS Code uses 1-based lines
                    } else {
                        1
                    };

                    eprintln!(
                        "📊 Stack trace: logical PC={}, physical line={}",
                        current_pc, physical_line
                    );

                    // Add current frame with the ACTUAL current line
                    frames.push(json!({
                        "id": 0,
                        "name": "main",
                        "line": physical_line,
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

        // Check for stopped event from execution thread
        if let Some(ref rx) = self.event_receiver {
            if let Ok((reason, _line)) = rx.recv_timeout(Duration::from_millis(500)) {
                if reason != "terminated" {
                    self.send_event(
                        "stopped".to_string(),
                        Some(json!({
                            "reason": reason,
                            "threadId": 1,
                            "allThreadsStopped": true
                        })),
                    );
                } else {
                    self.send_event("terminated".to_string(), None);
                }
            }
        }
    }

    pub fn handle_next(&mut self, seq: u64, command: String) {
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::StepOver);
                ctx.continue_requested = true;
            }
        }
        self.send_response(seq, command, true, None);

        // Check for stopped event from execution thread
        if let Some(ref rx) = self.event_receiver {
            if let Ok((reason, _line)) = rx.recv_timeout(Duration::from_millis(500)) {
                if reason != "terminated" {
                    self.send_event(
                        "stopped".to_string(),
                        Some(json!({
                            "reason": reason,
                            "threadId": 1,
                            "allThreadsStopped": true
                        })),
                    );
                } else {
                    self.send_event("terminated".to_string(), None);
                }
            }
        }
    }

    pub fn handle_step_in(&mut self, seq: u64, command: String) {
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::StepInto);
                ctx.continue_requested = true;
            }
        }
        self.send_response(seq, command, true, None);

        // Check for stopped event from execution thread
        if let Some(ref rx) = self.event_receiver {
            if let Ok((reason, _line)) = rx.recv_timeout(Duration::from_millis(500)) {
                if reason != "terminated" {
                    self.send_event(
                        "stopped".to_string(),
                        Some(json!({
                            "reason": reason,
                            "threadId": 1,
                            "allThreadsStopped": true
                        })),
                    );
                } else {
                    self.send_event("terminated".to_string(), None);
                }
            }
        }
    }

    pub fn handle_step_out(&mut self, seq: u64, command: String) {
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::StepOut);
                ctx.continue_requested = true;
            }
        }
        self.send_response(seq, command, true, None);

        // Check for stopped event from execution thread
        if let Some(ref rx) = self.event_receiver {
            if let Ok((reason, _line)) = rx.recv_timeout(Duration::from_millis(500)) {
                if reason != "terminated" {
                    self.send_event(
                        "stopped".to_string(),
                        Some(json!({
                            "reason": reason,
                            "threadId": 1,
                            "allThreadsStopped": true
                        })),
                    );
                } else {
                    self.send_event("terminated".to_string(), None);
                }
            }
        }
    }

    pub fn handle_pause(&mut self, seq: u64, command: String) {
        // Set the mode to StepInto so it stops at the next line
        if let Some(ctx_arc) = &self.context {
            if let Ok(mut ctx) = ctx_arc.lock() {
                ctx.set_mode(RunMode::StepInto);
                // Don't set continue_requested = true, leave it false
                // This will make the execution thread stop at the next line
            }
        }

        self.send_response(seq, command, true, None);

        // Send a stopped event
        self.send_event(
            "stopped".to_string(),
            Some(json!({
                "reason": "pause",
                "threadId": 1,
                "allThreadsStopped": true
            })),
        );
    }
}

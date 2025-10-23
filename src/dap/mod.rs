mod protocol;
mod server;

use serde_json::json;
use std::fs;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

pub use protocol::DapMessageContent;
pub use server::DapServer;

pub fn run_dap_mode() -> io::Result<()> {
    eprintln!("DAP server starting...");

    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\temp\\batch-debugger-vscode.log")
        .ok();

    if let Some(ref mut f) = log {
        writeln!(f, "DAP mode entered").ok();
    }

    let mut server = DapServer::new();
    let mut msg_count = 0;

    loop {
        // CRITICAL: Poll for output from execution thread
        server.check_and_send_output();

        // CRITICAL: Poll for stopped events from execution thread
        // Collect events first, then process them to avoid borrow checker issues
        let mut events = Vec::new();
        if let Some(ref rx) = server.event_receiver {
            while let Ok((reason, line)) = rx.try_recv() {
                events.push((reason, line));
            }
        }

        // Now process the events
        for (reason, _line) in events {
            if let Some(ref mut f) = log {
                writeln!(f, "📥 Event received: {}", reason).ok();
                f.flush().ok();
            }

            if reason != "terminated" {
                server.send_event(
                    "stopped".to_string(),
                    Some(json!({
                        "reason": reason,
                        "threadId": 1,
                        "allThreadsStopped": true
                    })),
                );
                eprintln!("📤 Sent stopped event: {}", reason);
            } else {
                eprintln!("📤 Sending terminated event");
                server.send_event("terminated".to_string(), None);
            }
        }

        // Try to read a DAP message (non-blocking)
        if let Some(msg) = server.try_read_message() {
            msg_count += 1;

            if let Some(ref mut f) = log {
                writeln!(f, "✓ Received message #{}: {:?}", msg_count, msg.content).ok();
                f.flush().ok();
            }

            eprintln!("📨 Received: {:?}", msg.content);

            match msg.content {
                DapMessageContent::Request { command, arguments } => match command.as_str() {
                    "initialize" => {
                        if let Some(ref mut f) = log {
                            writeln!(f, "Handling initialize").ok();
                        }
                        eprintln!("🔧 Handling initialize");
                        server.handle_initialize(msg.seq, command);
                    }
                    "launch" | "attach" => {
                        if let Some(ref mut f) = log {
                            writeln!(f, "Handling launch").ok();
                        }
                        eprintln!("🚀 Handling launch");
                        server.handle_launch(msg.seq, command, arguments);
                    }
                    "setBreakpoints" => {
                        server.handle_set_breakpoints(msg.seq, command, arguments);
                    }
                    "configurationDone" => {
                        server.send_response(msg.seq, command, true, None);
                    }
                    "threads" => {
                        server.handle_threads(msg.seq, command);
                    }
                    "stackTrace" => {
                        server.handle_stack_trace(msg.seq, command);
                    }
                    "scopes" => {
                        server.handle_scopes(msg.seq, command);
                    }
                    "variables" => {
                        server.handle_variables(msg.seq, command, arguments);
                    }
                    "continue" => {
                        server.handle_continue(msg.seq, command);
                    }
                    "next" => {
                        server.handle_next(msg.seq, command);
                    }
                    "stepIn" => {
                        server.handle_step_in(msg.seq, command);
                    }
                    "stepOut" => {
                        server.handle_step_out(msg.seq, command);
                    }
                    "pause" => {
                        eprintln!("Handling pause");
                        server.handle_pause(msg.seq, command);
                    }
                    "disconnect" => {
                        server.send_response(msg.seq, command, true, None);
                        break;
                    }
                    _ => {
                        eprintln!("⚠️  Unhandled DAP command: {}", command);
                        server.send_response(msg.seq, command, false, None);
                    }
                },
                _ => {
                    eprintln!("📬 Non-request message");
                }
            }
        }

        // Small sleep to prevent busy-waiting
        thread::sleep(Duration::from_millis(10));
    }

    if let Some(ref mut f) = log {
        writeln!(f, "DAP mode exiting").ok();
    }

    Ok(())
}

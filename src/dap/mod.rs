mod protocol;
mod server;

use std::fs;
use std::io::{self, Write};

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
        msg_count += 1;

        if let Some(ref mut f) = log {
            writeln!(f, "Waiting for message #{}...", msg_count).ok();
            f.flush().ok();
        }

        if let Some(msg) = server.read_message() {
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
        } else {
            if let Some(ref mut f) = log {
                writeln!(f, "✗ No message received").ok();
                f.flush().ok();
            }
        }
    }

    if let Some(ref mut f) = log {
        writeln!(f, "DAP mode exiting").ok();
    }

    Ok(())
}

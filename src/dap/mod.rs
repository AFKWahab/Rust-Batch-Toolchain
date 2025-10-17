mod protocol;
mod server;

use std::io;

pub use protocol::DapMessageContent;
pub use server::DapServer;

pub fn run_dap_mode() -> io::Result<()> {
    let mut server = DapServer::new();

    loop {
        if let Some(msg) = server.read_message() {
            match msg.content {
                DapMessageContent::Request { command, arguments } => match command.as_str() {
                    "initialize" => {
                        server.handle_initialize(msg.seq, command);
                    }
                    "launch" | "attach" => {
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
                _ => {}
            }
        }
    }

    Ok(())
}

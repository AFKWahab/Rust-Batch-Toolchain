mod protocol;
mod server;

use std::io;

pub use protocol::DapMessageContent;
pub use server::DapServer;

pub fn run_dap_mode() -> io::Result<()> {
    let mut server = DapServer::new();

    server.send_event("initialized".to_string(), None);

    loop {
        if let Some(msg) = server.read_message() {
            match msg.content {
                DapMessageContent::Request {
                    command,
                    arguments: _,
                } => match command.as_str() {
                    "initialize" => {
                        server.handle_initialize(msg.seq, command);
                    }
                    "launch" | "attach" => {
                        server.handle_launch(msg.seq, command);
                    }
                    "setBreakpoints" => {
                        server.handle_set_breakpoints(msg.seq, command);
                    }
                    "continue" | "next" | "stepIn" | "stepOut" => {
                        server.handle_step(msg.seq, command);
                    }
                    "disconnect" => {
                        server.send_response(msg.seq, command, true, None);
                        break;
                    }
                    _ => {
                        server.send_response(msg.seq, command, false, None);
                    }
                },
                _ => {}
            }
        }
    }

    Ok(())
}

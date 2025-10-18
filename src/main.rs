mod dap;
mod debugger;
mod executor;
mod parser;

use std::fs;
use std::io::{self, Write};

fn main() -> io::Result<()> {
    // Log to file
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\temp\\batch-debugger-vscode.log")
        .ok();

    if let Some(ref mut f) = log {
        writeln!(
            f,
            "\n=== DEBUGGER STARTED at {:?} ===",
            std::time::SystemTime::now()
        )
        .ok();
    }

    let args: Vec<String> = std::env::args().collect();

    if let Some(ref mut f) = log {
        writeln!(f, "Args: {:?}", args).ok();
    }

    let dap_mode = args
        .iter()
        .any(|arg| arg == "--dap" || arg == "--debug-adapter");

    if dap_mode {
        if let Some(ref mut f) = log {
            writeln!(f, "Starting DAP mode").ok();
        }
        eprintln!("Starting in DAP mode...");
        dap::run_dap_mode()?;
    } else {
        eprintln!("Starting in interactive mode...");
        run_interactive_mode()?;
    }

    if let Some(ref mut f) = log {
        writeln!(f, "=== DEBUGGER EXITING ===").ok();
    }

    Ok(())
}

fn run_interactive_mode() -> io::Result<()> {
    let contents = fs::read_to_string("test.bat").expect("Could not read test.bat");
    let physical_lines: Vec<&str> = contents.lines().collect();

    let pre = parser::preprocess_lines(&physical_lines);
    let labels_phys = parser::build_label_map(&physical_lines);

    let session = debugger::CmdSession::start()?;
    let mut ctx = debugger::DebugContext::new(session);

    ctx.set_mode(debugger::RunMode::StepInto);

    executor::run_debugger(&mut ctx, &pre, &labels_phys)?;

    let _ = ctx.session_mut().run("ENDLOCAL & exit");
    Ok(())
}

use crate::debugger::{leave_context, DebugContext, Frame, RunMode};
use crate::parser::{is_comment, normalize_whitespace, PreprocessResult};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// DAP-specific executor that sends stopped events instead of interactive prompts
pub fn run_debugger_dap(
    ctx_arc: Arc<Mutex<DebugContext>>,
    pre: &PreprocessResult,
    labels_phys: &HashMap<String, usize>,
) -> io::Result<()> {
    let mut pc: usize = 0;
    let mut step_depth: Option<usize> = None;

    'run: loop {
        // EOF unwinding
        while pc >= pre.logical.len() {
            let mut ctx = match ctx_arc.lock() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("❌ Failed to lock context: {}", e);
                    break 'run;
                }
            };
            match leave_context(&mut ctx.call_stack) {
                Some(next_pc) => pc = next_pc,
                None => break 'run,
            }
        }

        let ll = &pre.logical[pc];
        let raw = ll.text.as_str();
        let line = normalize_whitespace(raw.trim());
        let line_upper = line.to_uppercase();

        // Skip empty / comment / label lines (but NOT @echo off)
        if line.trim().starts_with(':') {
            pc += 1;
            continue;
        }

        // Skip REM and :: comments
        if line_upper.starts_with("REM ") || line.trim().starts_with("::") {
            pc += 1;
            continue;
        }

        // Check if we should stop at this line
        let should_stop = {
            let ctx = match ctx_arc.lock() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("❌ Failed to lock context: {}", e);
                    break 'run;
                }
            };

            match ctx.mode() {
                RunMode::Continue => ctx.should_stop_at(pc),
                RunMode::StepInto => true,
                RunMode::StepOver => {
                    if let Some(target_depth) = step_depth {
                        ctx.call_stack.len() <= target_depth
                    } else {
                        true
                    }
                }
                RunMode::StepOut => ctx.should_stop_at(pc),
            }
        };

        // If we should stop, pause and wait for DAP to tell us to continue
        if should_stop {
            eprintln!(
                "🛑 DAP: Stopped at line {} (phys {}): {}",
                pc,
                ll.phys_start + 1,
                raw
            );

            // Reset the continue flag
            {
                let mut ctx = match ctx_arc.lock() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("❌ Failed to lock context: {}", e);
                        break 'run;
                    }
                };
                ctx.continue_requested = false;
            }

            // Wait for continue_requested to be set to true
            let mut wait_count = 0;
            loop {
                std::thread::sleep(Duration::from_millis(50));
                wait_count += 1;

                // Timeout after 5 minutes (6000 * 50ms)
                if wait_count > 6000 {
                    eprintln!("⚠️ Timeout waiting for step command");
                    break 'run;
                }

                let ctx = match ctx_arc.lock() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("❌ Failed to lock context during wait: {}", e);
                        break 'run;
                    }
                };

                if ctx.continue_requested {
                    eprintln!("✓ Continue requested, mode: {:?}", ctx.mode());
                    // Update step_depth based on mode
                    match ctx.mode() {
                        RunMode::Continue => {
                            step_depth = None;
                        }
                        RunMode::StepOver => {
                            step_depth = Some(ctx.call_stack.len());
                        }
                        RunMode::StepInto => {
                            step_depth = None;
                        }
                        RunMode::StepOut => {
                            step_depth = None;
                        }
                    }
                    break;
                }
            }
        }

        // Execute the line (same logic as interactive mode)
        {
            let mut ctx = match ctx_arc.lock() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("❌ Failed to lock context for execution: {}", e);
                    break 'run;
                }
            };

            // Handle SETLOCAL
            if line_upper.starts_with("SETLOCAL") {
                ctx.handle_setlocal();
                let (out, code) = ctx.run_command(&line)?;
                if !out.trim().is_empty() {
                    print!("{}", out);
                }
                ctx.last_exit_code = code;
                pc += 1;
                continue;
            }

            // Handle ENDLOCAL
            if line_upper.starts_with("ENDLOCAL") {
                ctx.handle_endlocal();
                let (out, code) = ctx.run_command(&line)?;
                if !out.trim().is_empty() {
                    print!("{}", out);
                }
                ctx.last_exit_code = code;
                pc += 1;
                continue;
            }

            // CALL :label
            if line_upper.starts_with("CALL ") {
                let rest = &line[5..].trim();
                let mut lexer = shlex::Shlex::new(rest);
                let first = lexer.next().unwrap_or_default();
                let label_key = first.trim_start_matches(':').to_lowercase();
                let args: Vec<String> = lexer.collect();

                if let Some(&phys_target) = labels_phys.get(&label_key) {
                    let logical_target = pre.phys_to_logical[phys_target];
                    ctx.call_stack.push(Frame::new(pc + 1, Some(args)));
                    pc = logical_target;
                } else {
                    eprintln!("❌ CALL to unknown label: {}", label_key);
                    break 'run;
                }
                continue;
            }

            // EXIT /B
            if line_upper.starts_with("EXIT /B") {
                let rest = &line[7..].trim();
                let code: i32 = rest.parse::<i32>().unwrap_or(0);
                ctx.last_exit_code = code;

                match leave_context(&mut ctx.call_stack) {
                    Some(next_pc) => pc = next_pc,
                    None => break 'run,
                }
                continue;
            }

            // GOTO
            if line_upper.starts_with("GOTO ") {
                let rest = &line[5..].trim();
                let label_key = rest
                    .trim_start_matches(':')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_lowercase();

                if label_key == "eof" {
                    match leave_context(&mut ctx.call_stack) {
                        Some(next_pc) => pc = next_pc,
                        None => break 'run,
                    }
                    continue;
                }

                if let Some(&phys_target) = labels_phys.get(&label_key) {
                    let logical_target = pre.phys_to_logical[phys_target];
                    pc = logical_target;
                } else {
                    eprintln!("❌ GOTO to unknown label: {}", label_key);
                    break 'run;
                }
                continue;
            }

            // Execute normal command
            eprintln!("▶️ Executing: {}", line);
            ctx.track_set_command(&line);

            match ctx.run_command(&line) {
                Ok((out, code)) => {
                    if !out.trim().is_empty() {
                        print!("{}", out);
                    }
                    ctx.last_exit_code = code;
                }
                Err(e) => {
                    eprintln!("❌ Command execution error: {}", e);
                    break 'run;
                }
            }
        }

        pc += 1;
    }

    eprintln!("✅ DAP: Script execution completed");
    Ok(())
}

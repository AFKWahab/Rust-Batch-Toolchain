use crate::debugger::{leave_context, DebugContext, Frame, RunMode};
use crate::parser::{
    is_comment, normalize_whitespace, split_composite_command, CommandOp, PreprocessResult,
};
use std::collections::HashMap;
use std::io::{self, Write};

pub fn run_debugger(
    ctx: &mut DebugContext,
    pre: &PreprocessResult,
    labels_phys: &HashMap<String, usize>,
) -> io::Result<()> {
    let mut pc: usize = 0;

    'run: loop {
        // EOF unwinding
        while pc >= pre.logical.len() {
            match leave_context(&mut ctx.call_stack) {
                Some(next_pc) => {
                    pc = next_pc;
                }
                None => {
                    break 'run;
                }
            }
        }

        let ll = &pre.logical[pc];
        let raw = ll.text.as_str();
        let line = normalize_whitespace(raw.trim());
        let line_upper = line.to_uppercase();

        // Skip empty / comment lines
        if is_comment(&line) {
            pc += 1;
            continue;
        }

        // Skip label definition lines
        if line.trim().starts_with(':') {
            pc += 1;
            continue;
        }

        // Check if we should stop at this line
        if ctx.should_stop_at(pc) {
            eprintln!(
                "\n⏸️  Stopped at logical line {} (phys line {})",
                pc,
                ll.phys_start + 1
            );
            eprintln!("    {}", raw);
            ctx.print_call_stack(&pre.logical);

            eprintln!("\nCommands: (c)ontinue, (n)ext/stepOver, (s)tepIn, (o)ut/stepOut, (b)reakpoint, (q)uit");
            eprint!("> ");
            io::stderr().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let cmd = input.trim();

            match cmd {
                "c" | "continue" => ctx.handle_step_command("continue"),
                "n" | "next" | "stepOver" => ctx.handle_step_command("stepOver"),
                "s" | "stepIn" | "stepInto" => ctx.handle_step_command("stepInto"),
                "o" | "out" | "stepOut" => ctx.handle_step_command("stepOut"),
                "q" | "quit" => break 'run,
                cmd if cmd.starts_with("b ") => {
                    if let Ok(line_num) = cmd[2..].trim().parse::<usize>() {
                        ctx.add_breakpoint(line_num);
                    }
                }
                _ => eprintln!("Unknown command: {}", cmd),
            }
        }

        // After stepping once in StepOver/StepInto, revert to Continue mode
        if matches!(ctx.mode(), RunMode::StepOver | RunMode::StepInto) {
            ctx.set_mode(RunMode::Continue);
        }

        // PAUSE command
        if line_upper == "PAUSE" {
            eprintln!("\n⏸  Press Enter to continue...");
            let mut buf = String::new();
            io::stdin().read_line(&mut buf)?;
            pc += 1;
            continue;
        }

        // CALL :label
        if line_upper.starts_with("CALL ") {
            let rest = &line[5..].trim();
            let label_key = rest
                .trim_start_matches(':')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_lowercase();

            if let Some(&phys_target) = labels_phys.get(&label_key) {
                let logical_target = pre.phys_to_logical[phys_target];

                if ctx.mode() == RunMode::StepOver {
                    eprintln!("\n📞 CALL to :{} (stepping over)", label_key);
                    ctx.set_mode(RunMode::Continue);
                }

                ctx.call_stack.push(Frame {
                    return_pc: pc + 1,
                    args: None,
                    locals: None,
                });

                eprintln!(
                    "\n📞 CALL to :{} (jumping to logical line {})",
                    label_key, logical_target
                );
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

            eprintln!("\n🚪 EXIT /B {} (returning from subroutine)", code);

            match leave_context(&mut ctx.call_stack) {
                Some(next_pc) => {
                    pc = next_pc;
                }
                None => break 'run,
            }
            continue;
        }

        // GOTO :EOF
        if line_upper == "GOTO :EOF" {
            eprintln!("\n↩️  GOTO :EOF (returning from subroutine)");

            match leave_context(&mut ctx.call_stack) {
                Some(next_pc) => {
                    pc = next_pc;
                }
                None => break 'run,
            }
            continue;
        }

        // GOTO label
        if line_upper.starts_with("GOTO ") {
            let rest = &line[5..].trim();
            let label_key = rest
                .trim_start_matches(':')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_lowercase();

            if let Some(&phys_target) = labels_phys.get(&label_key) {
                let logical_target = pre.phys_to_logical[phys_target];
                eprintln!(
                    "\n➡️  GOTO :{} (jumping to logical line {})",
                    label_key, logical_target
                );
                pc = logical_target;
            } else {
                eprintln!("❌ GOTO to unknown label: {}", label_key);
                break 'run;
            }
            continue;
        }

        // Execute command
        eprintln!(
            "\n▶️  [Lines {}-{}] depth={} group={:?}",
            ll.phys_start + 1,
            ll.phys_end + 1,
            ll.group_depth,
            ll.group_id
        );
        eprintln!("    {}", raw);

        ctx.track_set_command(&line);

        let parts = split_composite_command(&line);

        for (i, part) in parts.iter().enumerate() {
            if part.text.trim().is_empty() {
                continue;
            }

            let should_execute = match (i, ctx.last_exit_code) {
                (0, _) => true,
                (_, code) => {
                    let prev_op = parts[i - 1].op;
                    match prev_op {
                        Some(CommandOp::Unconditional) => true,
                        Some(CommandOp::And) => code == 0,
                        Some(CommandOp::Or) => code != 0,
                        None => true,
                    }
                }
            };

            if should_execute {
                if parts.len() > 1 {
                    eprintln!("    ├─ Part {}: {}", i + 1, part.text);
                }

                let (out, code) = ctx.run_command(&part.text)?;
                if !out.trim().is_empty() {
                    print!("{}", out);
                }

                ctx.last_exit_code = code;
                eprintln!("    └─ exit code: {}", code);
            } else {
                eprintln!("    ├─ Part {} skipped (condition failed)", i + 1);
            }
        }

        pc += 1;
    }

    eprintln!("\n✅ Script execution completed");
    ctx.print_call_stack(&pre.logical);
    ctx.print_variables();

    Ok(())
}

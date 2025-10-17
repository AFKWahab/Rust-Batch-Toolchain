use crate::debugger::{leave_context, DebugContext, Frame, RunMode};
use crate::parser::{
    is_comment, normalize_whitespace, split_composite_command, CommandOp, PreprocessResult,
};
use std::collections::HashMap;
use std::io::{self, Write};

/// Compute net parenthesis delta for a line, honoring quotes and ^ escapes
fn paren_delta(line: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '^' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quotes = !in_quotes;
            continue;
        }
        if !in_quotes {
            match ch {
                '(' => delta += 1,
                ')' => delta -= 1,
                _ => {}
            }
        }
    }
    delta
}

/// Minimal expander for %1..%9 and %~1..%~9 (strip surrounding quotes)
fn expand_positional_args(mut text: String, args: &[String]) -> String {
    // Replace higher numbers first to avoid %10 matching %1
    for i in (1..=9).rev() {
        let idx = i - 1;
        let val = args.get(idx).cloned().unwrap_or_default();
        let unquoted = val.trim_matches('"').to_string();

        text = text.replace(&format!("%~{}", i), &unquoted);
        text = text.replace(&format!("%{}", i), &val);
    }
    text
}

pub fn run_debugger(
    ctx: &mut DebugContext,
    pre: &PreprocessResult,
    labels_phys: &HashMap<String, usize>,
) -> io::Result<()> {
    let mut pc: usize = 0;
    let mut step_depth: Option<usize> = None; // Track depth for StepOver

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

        // Detect potential block start (IF ... ( or FOR ... ()
        let is_block_start = (line_upper.starts_with("IF ") || line_upper.starts_with("FOR "))
            && paren_delta(raw) > 0;

        // Determine if we should stop at this line
        let should_stop = match ctx.mode() {
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
        };

        // Stop point UI
        if should_stop {
            eprintln!(
                "\n🔍 Stopped at logical line {} (phys line {})",
                pc,
                ll.phys_start + 1
            );
            eprintln!("    {}", raw);

            if is_block_start {
                eprintln!("    [This is the start of a multi-line block]");
            }

            ctx.print_call_stack(&pre.logical);

            'prompt: loop {
                eprintln!("\nCommands: (c)ontinue, (n)ext/stepOver, (s)tepIn, (o)ut/stepOut, (b)reakpoint <line>, (q)uit");
                eprint!("> ");
                io::stderr().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let cmd = input.trim();

                match cmd {
                    "c" | "continue" => {
                        ctx.handle_step_command("continue");
                        step_depth = None;
                        break 'prompt;
                    }
                    "n" | "next" | "stepOver" => {
                        ctx.handle_step_command("stepOver");
                        step_depth = Some(ctx.call_stack.len());
                        break 'prompt;
                    }
                    "s" | "stepIn" | "stepInto" => {
                        ctx.handle_step_command("stepInto");
                        step_depth = None;
                        break 'prompt;
                    }
                    "o" | "out" | "stepOut" => {
                        ctx.handle_step_command("stepOut");
                        step_depth = None;
                        break 'prompt;
                    }
                    "q" | "quit" => break 'run,
                    cmd if cmd.starts_with("b ") => {
                        if let Ok(line_num) = cmd[2..].trim().parse::<usize>() {
                            ctx.add_breakpoint(line_num);
                        } else {
                            eprintln!("❌ Invalid line number");
                        }
                    }
                    "" => {
                        // Empty input - step into by default
                        ctx.handle_step_command("stepInto");
                        step_depth = None;
                        break 'prompt;
                    }
                    _ => {
                        eprintln!("❓ Unknown command: {}", cmd);
                    }
                }
            }
        }

        // PAUSE command (interactive)
        if line_upper == "PAUSE" {
            eprintln!("\n⏸  Press Enter to continue...");
            let mut buf = String::new();
            io::stdin().read_line(&mut buf)?;
            pc += 1;
            continue;
        }

        // CALL :label [args...]
        if line_upper.starts_with("CALL ") {
            let rest = &line[5..].trim();

            // Use shlex to split once: first token is label, remaining tokens are args (quotes preserved)
            let mut lexer = shlex::Shlex::new(rest);
            let first = lexer.next().unwrap_or_default();
            let label_key = first.trim_start_matches(':').to_lowercase();
            let args: Vec<String> = lexer.collect();

            if let Some(&phys_target) = labels_phys.get(&label_key) {
                let logical_target = pre.phys_to_logical[phys_target];

                ctx.call_stack.push(Frame::new(pc + 1, Some(args)));

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

        // Handle block constructs (IF, FOR with parentheses)
        if is_block_start {
            let mut block_lines = vec![raw.to_string()];
            let mut block_pc = pc + 1;
            let mut balance = paren_delta(raw);

            eprintln!("\n📦 Collecting block starting at line {}", pc);

            while balance > 0 && block_pc < pre.logical.len() {
                let b = &pre.logical[block_pc];
                block_lines.push(b.text.clone());
                balance += paren_delta(&b.text);
                block_pc += 1;
            }

            // Expand positional args if inside a subroutine
            if let Some(frame) = ctx.call_stack.last() {
                if let Some(a) = &frame.args {
                    for l in &mut block_lines {
                        *l = expand_positional_args(l.clone(), a);
                    }
                }
            }

            let (out, code) = ctx.session_mut().run_batch_block(&block_lines)?;
            if !out.trim().is_empty() {
                print!("{}", out);
            }
            ctx.last_exit_code = code;
            eprintln!("    └─ block exit code: {}", code);

            pc = block_pc;
            continue;
        }

        // Execute single-line command
        if !should_stop {
            eprintln!(
                "\n▶️  [Lines {}-{}] depth={} group={:?}",
                ll.phys_start + 1,
                ll.phys_end + 1,
                ll.group_depth,
                ll.group_id
            );
            eprintln!("    {}", raw);
        }

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
                let mut exec_text = part.text.clone();
                if let Some(frame) = ctx.call_stack.last() {
                    if let Some(a) = &frame.args {
                        exec_text = expand_positional_args(exec_text, a);
                    }
                }

                if parts.len() > 1 {
                    eprintln!("    ├─ Part {}: {}", i + 1, exec_text);
                }

                ctx.track_set_command(&exec_text);

                let (out, code) = ctx.run_command(&exec_text)?;
                if !out.trim().is_empty() {
                    print!("{}", out);
                }

                ctx.last_exit_code = code;
                if !should_stop {
                    eprintln!("    └─ exit code: {}", code);
                }
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

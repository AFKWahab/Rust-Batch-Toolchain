use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const SENTINEL: &str = "__CMD_DONE__";

struct CmdSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

struct Frame {
    return_pc: usize,
    args: Option<Vec<String>>,
    locals: Option<Vec<String>>,
}

impl CmdSession {
    fn start() -> io::Result<Self> {
        // /Q = quiet (don't echo), /K = keep session open, /V:ON = delayed expansion for !ERRORLEVEL!
        let mut child = Command::new("cmd")
            .args(["/Q", "/K", "SETLOCAL EnableDelayedExpansion & PROMPT $G"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().expect("no stdin");
        let stdout = child.stdout.take().expect("no stdout");

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// Run one command in the persistent cmd; returns (combined_output, exit_code)
    fn run(&mut self, cmd: &str) -> io::Result<(String, i32)> {
        let wrapped = format!("{cmd} 2>&1 & echo {SENTINEL} !ERRORLEVEL!\r\n");
        self.stdin.write_all(wrapped.as_bytes())?;
        self.stdin.flush()?;

        let mut output = String::new();
        let mut exit_code = 0;

        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                // child ended
                break;
            }
            let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
            if let Some(rest) = trimmed.strip_prefix(SENTINEL) {
                if let Ok(code) = rest.trim().parse::<i32>() {
                    exit_code = code;
                }
                break;
            } else {
                output.push_str(&line);
            }
        }

        Ok((output, exit_code))
    }
}

// Scan labels (case-insensitive)
fn build_label_map(lines: &[&str]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with(':') && t.len() > 1 {
            map.insert(t[1..].trim().to_lowercase(), i);
        }
    }
    map
}

/// Helper: unwind the current context at EOF.
/// Returns Some(next_pc) when returning to a caller,
/// or None when we should end the script (top-level EOF).
fn leave_context(call_stack: &mut Vec<Frame>) -> Option<usize> {
    if let Some(frame) = call_stack.pop() {
        Some(frame.return_pc) // implicit "GOTO :EOF" from subroutine
    } else {
        None // no caller: end script
    }
}

fn main() -> io::Result<()> {
    // Read the batch file
    let contents = fs::read_to_string("test.bat").expect("Could not read test.bat");
    let lines: Vec<&str> = contents.lines().collect();

    // Pre-scan labels for GOTO
    let labels = build_label_map(&lines);

    // Start persistent cmd session
    let mut session = CmdSession::start()?;
    let mut pc: usize = 0;
    let mut call_stack: Vec<Frame> = Vec::new();
    let mut _last_exit_code: i32 = 0;

    'run: loop {
        // --- EOF unwinding: while pc is out of bounds, keep returning to callers.
        while pc >= lines.len() {
            match leave_context(&mut call_stack) {
                Some(next_pc) => {
                    pc = next_pc;
                    // loop back to re-check bounds with the new pc
                }
                None => {
                    // Truly top-level EOF → end script
                    break 'run;
                }
            }
        }

        // Safe to fetch the current line now
        let raw = lines[pc];
        let line = raw.trim();

        // Skip empty / comment lines
        if line.is_empty() || line.starts_with("REM") || line.starts_with("::") {
            pc += 1;
            continue;
        }

        // Skip label definition lines (":label")
        if line.starts_with(':') {
            pc += 1;
            continue;
        }

        // ---- CALL :label
        if let Some(rest) = line.strip_prefix("CALL ") {
            let label = rest.trim().trim_start_matches(':').to_lowercase();
            if let Some(&target) = labels.get(&label) {
                call_stack.push(Frame {
                    return_pc: pc + 1,
                    args: None,
                    locals: None,
                });
                pc = target; // jump to label line (next iter will skip the ':' line)
            } else {
                eprintln!("CALL to unknown label: {label}");
                break;
            }
            continue;
        }

        // ---- EXIT /B [n]
        if let Some(rest) = line.strip_prefix("EXIT /B") {
            let _code: i32 = rest.trim().parse::<i32>().unwrap_or(0);
            // Note: EXIT /B N sets ERRORLEVEL in real cmd; you can mirror in UI if desired.
            match leave_context(&mut call_stack) {
                Some(next_pc) => {
                    pc = next_pc;
                }
                None => break, // end script
            }
            continue;
        }

        // ---- GOTO :EOF (return or end script)
        if line.eq_ignore_ascii_case("GOTO :EOF") {
            match leave_context(&mut call_stack) {
                Some(next_pc) => {
                    pc = next_pc;
                }
                None => break, // end script
            }
            continue;
        }

        // ---- GOTO label
        if let Some(rest) = line.strip_prefix("GOTO ") {
            let label = rest.trim().to_lowercase();
            if let Some(&target) = labels.get(&label) {
                pc = target; // jump to label line (next iter will skip ':' line)
            } else {
                eprintln!("GOTO to unknown label: {label}");
                break;
            }
            continue;
        }

        // ---- Execute everything else inside persistent cmd
        println!("Executing line {pc}: {raw}");
        let (out, code) = session.run(line)?;
        if !out.is_empty() {
            print!("{out}");
        }
        println!("(exit code: {code})");
        _last_exit_code = code;

        // Advance to next line; EOF unwinding happens at top of loop
        pc += 1;
    }

    // Clean exit for the cmd session
    let _ = session.run("ENDLOCAL & exit");

    Ok(())
}

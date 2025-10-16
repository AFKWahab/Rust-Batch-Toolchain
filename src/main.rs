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

// Look through every line, if it starts with a colon, it's a label, so we take its index and save the string to it with a hashmap
fn build_label_map(lines: &[&str]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with(':') && t.len() > 1 {
            // label is everything after the leading ':'
            map.insert(t[1..].trim().to_string(), i);
        }
    }
    map
}

fn main() -> io::Result<()> {
    // Read the batch file
    let contents = fs::read_to_string("test.bat").expect("Could not read test.bat");
    let lines: Vec<&str> = contents.lines().collect();

    // Pre-scan labels for GOTO
    let labels = build_label_map(&lines);
    // Start persistent cmd session

    let mut session = CmdSession::start()?;
    let mut pc = 0usize;
    let mut call_stack: Vec<Frame> = Vec::new();
    let mut last_exit_code: i32 = 0;

    while pc < lines.len() {
        let raw = lines[pc];
        let line = raw.trim();

        // Skip empty / REM lines
        if line.is_empty() || line.starts_with("REM") || line.starts_with("::") {
            pc += 1;
            continue;
        }

        // Skip label definition lines (":label")
        if line.starts_with(':') {
            pc += 1;
            continue;
        }

        /* FROM HERE AND DOWNARDS UNTIL THE 'END CALL' COMMENT, we are handling the call stack */
        // Handle call_stack, by saving current line, in the Vec<Frame>, and then jumping to the line
        if let Some(rest) = line.strip_prefix("CALL ") {
            let label = rest.trim().trim_start_matches(':');
            if let Some(&target) = labels.get(label) {
                call_stack.push(Frame {
                    return_pc: pc + 1,
                    args: None,
                    locals: None,
                });
                pc = target; // jump return_pc: pc (next loop iteration will skip the ':' line)
            } else {
                eprintln!("CALL to unknown label: {label}");
                break;
            }
            continue;
        }

        // Handle EXIT /B [n]
        if let Some(rest) = line.strip_prefix("EXIT /B") {
            let code = rest.trim().parse::<i32>().unwrap_or(0);
            // Pop the current frame (if any)
            if let Some(frame) = call_stack.pop() {
                pc = frame.return_pc;
            } else {
                // Stack empty: top-level EXIT /B means end script
                pc = lines.len();
            }
            continue;
        }

        // Handle GOTO :EOF (return from subroutine or end script)
        if line.trim().eq_ignore_ascii_case("GOTO :EOF") {
            if let Some(frame) = call_stack.pop() {
                // Return from subroutine
                pc = frame.return_pc;
            } else {
                // Top-level: no caller left → end script
                pc = lines.len();
            }
            continue;
        }

        /*
         * END CALL
         */
        // Handle GOTO locally by changing the program counter (don't send to cmd)
        if let Some(rest) = line.strip_prefix("GOTO ") {
            let label = rest.trim();
            if let Some(&target) = labels.get(label) {
                pc = target; // jump to label line (next loop iteration will skip the ':' line)
            } else {
                eprintln!("GOTO to unknown label: {label}");
                break;
            }
            continue;
        }

        // Everything else: run inside the persistent cmd
        println!("Executing line {pc}: {raw}");
        let (out, code) = session.run(line)?;
        if !out.is_empty() {
            print!("{out}");
        }
        println!("(exit code: {code})");

        pc += 1;
    }

    // Clean exit for the cmd session
    let _ = session.run("ENDLOCAL & exit");

    Ok(())
}

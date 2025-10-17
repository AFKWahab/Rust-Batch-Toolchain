use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const SENTINEL: &str = "__CMD_DONE__";

pub struct CmdSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl CmdSession {
    pub fn start() -> io::Result<Self> {
        // Enable delayed expansion globally so !VAR! works as expected.
        let mut child = Command::new("cmd")
            .args(["/V:ON", "/Q"]) // <— important change
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().expect("no stdin");
        let stdout = child.stdout.take().expect("no stdout");

        let mut session = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
        };

        // Send initial echo off to suppress prompts
        session.stdin.write_all(b"@echo off\r\n")?;
        session.stdin.flush()?;

        // Clear any initial output by reading available lines with a simple marker
        session.stdin.write_all(b"echo INITIALIZED\r\n")?;
        session.stdin.flush()?;

        let mut line = String::new();
        let timeout = Duration::from_secs(2);
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                break;
            }
            line.clear();
            match session.stdout.read_line(&mut line) {
                Ok(_) => {
                    if line.contains("INITIALIZED") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        Ok(session)
    }

    /// Check if a command needs multi-line input (has unclosed parentheses)
    fn needs_continuation(cmd: &str) -> bool {
        let mut paren_count = 0;
        let mut in_quotes = false;
        let mut escaped = false;

        for ch in cmd.chars() {
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
                    '(' => paren_count += 1,
                    ')' => paren_count -= 1,
                    _ => {}
                }
            }
        }

        paren_count > 0
    }

    /// Execute a multi-line block as a *real batch file* preserving CRLFs and batch parsing rules.
    pub fn run_batch_block(&mut self, lines: &[String]) -> io::Result<(String, i32)> {
        let temp_batch = "__temp_block__.bat";

        // Preserve original line structure; batch parsing requires CRLF boundaries.
        let mut body = String::from("@echo off\r\n");
        for l in lines {
            body.push_str(l);
            body.push_str("\r\n");
        }

        std::fs::write(temp_batch, body).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Execute via CALL so the session stays alive
        let (out, code) = self.run(&format!("call {}", temp_batch))?;

        // Best-effort cleanup; ignore errors
        let _ = self.run(&format!("del {} >nul 2>&1", temp_batch));

        Ok((out, code))
    }

    pub fn run(&mut self, cmd: &str) -> io::Result<(String, i32)> {
        // Special case for @echo off - it produces no output
        if cmd.trim().eq_ignore_ascii_case("@echo off")
            || cmd.trim().eq_ignore_ascii_case("echo off")
        {
            self.stdin.write_all(cmd.as_bytes())?;
            self.stdin.write_all(b"\r\n")?;
            self.stdin.flush()?;
            return Ok((String::new(), 0));
        }

        let debug_this = cmd.contains("set /a") || cmd.contains("COUNTER") || cmd.contains("if ");

        if debug_this {
            eprintln!("DEBUG: About to execute: '{}'", cmd);
        }

        // Check if this is a multi-line command (rare for single-line path)
        let is_multiline = Self::needs_continuation(cmd);

        if is_multiline {
            eprintln!("DEBUG: Detected multi-line command");
            // Write to a temporary batch file and execute it to preserve semantics
            let temp_batch = "__temp_cmd__.bat";
            std::fs::write(temp_batch, format!("@echo off\r\n{}\r\n", cmd))
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            // Execute the temp batch file
            self.stdin
                .write_all(format!("call {}\r\n", temp_batch).as_bytes())?;
            self.stdin.flush()?;

            // Clean up
            std::thread::sleep(Duration::from_millis(200));
            self.stdin
                .write_all(format!("del {} >nul 2>&1\r\n", temp_batch).as_bytes())?;
            self.stdin.flush()?;
        } else {
            // Send the command normally
            self.stdin.write_all(cmd.as_bytes())?;
            self.stdin.write_all(b"\r\n")?;
            self.stdin.flush()?;
        }

        // Give the command time to execute
        std::thread::sleep(Duration::from_millis(100));

        // Send echo command to force a newline and get the exit code
        self.stdin.write_all(b"echo.\r\n")?; // Force a blank line first
        let sentinel_cmd = format!("echo {}_%errorlevel%_END\r\n", SENTINEL);
        self.stdin.write_all(sentinel_cmd.as_bytes())?;
        self.stdin.flush()?;

        let mut output = String::new();
        let mut exit_code = 0;
        let timeout = Duration::from_secs(5);
        let start = Instant::now();
        let mut found_blank = false;
        let mut collecting = true;

        loop {
            // Check timeout
            if start.elapsed() > timeout {
                eprintln!("WARNING: Command timed out after 5 seconds");
                eprintln!("  Command was: {}", cmd);
                eprintln!("  Output collected so far: '{}'", output.trim());
                return Ok((output, 1));
            }

            let mut line = String::new();
            match self.stdout.read_line(&mut line) {
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Ok(_) => {
                    let trimmed = line.trim();

                    if debug_this {
                        eprintln!("DEBUG: Read line: '{}'", trimmed);
                    }

                    // Check for our sentinel
                    if trimmed.starts_with(SENTINEL) && trimmed.ends_with("_END") {
                        let prefix_len = SENTINEL.len() + 1;
                        let suffix_len = 4;
                        if trimmed.len() > prefix_len + suffix_len {
                            let code_str = &trimmed[prefix_len..trimmed.len() - suffix_len];
                            if let Ok(code) = code_str.parse::<i32>() {
                                exit_code = code;
                            }
                        }
                        break;
                    }

                    // Look for the blank line we inserted
                    if trimmed.is_empty() && !found_blank {
                        found_blank = true;
                        collecting = false;
                        continue;
                    }

                    // Collect output only before the blank line
                    if collecting && !trimmed.is_empty() {
                        output.push_str(&line);
                    }
                }
                Err(e) => {
                    eprintln!("DEBUG: Read error: {}", e);
                    return Err(e);
                }
            }
        }

        Ok((output, exit_code))
    }
}

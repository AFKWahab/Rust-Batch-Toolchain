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
        let mut child = Command::new("cmd")
            .args(["/Q"]) // Just quiet mode
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

        // For debugging problematic commands
        let debug_this = cmd.contains("set /a") || cmd.contains("COUNTER");

        if debug_this {
            eprintln!("DEBUG: About to execute: '{}'", cmd);
        }

        // Send the command followed by our sentinel
        // Use multiple newlines to ensure the echo command is on its own line
        self.stdin.write_all(cmd.as_bytes())?;
        self.stdin.write_all(b"\r\n")?;
        self.stdin.flush()?;

        // Small delay to ensure command completes
        std::thread::sleep(Duration::from_millis(50));

        // Now send the sentinel
        let sentinel_cmd = format!("echo {}_%errorlevel%_END\r\n", SENTINEL);
        self.stdin.write_all(sentinel_cmd.as_bytes())?;
        self.stdin.flush()?;

        let mut output = String::new();
        let mut exit_code = 0;
        let timeout = Duration::from_secs(3);
        let start = Instant::now();
        let mut lines_read = 0;

        loop {
            // Check timeout
            if start.elapsed() > timeout {
                eprintln!("WARNING: Command timed out after 3 seconds");
                eprintln!("  Command was: {}", cmd);
                eprintln!("  Output collected so far: '{}'", output.trim());
                // Return what we have with a non-zero exit code
                return Ok((output, 1));
            }

            let mut line = String::new();
            match self.stdout.read_line(&mut line) {
                Ok(0) => {
                    if debug_this {
                        eprintln!("DEBUG: EOF reached");
                    }
                    // Give it another chance with a small delay
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Ok(_) => {
                    lines_read += 1;
                    let trimmed = line.trim();

                    if debug_this {
                        eprintln!("DEBUG: Line {}: '{}'", lines_read, trimmed);
                    }

                    // Check if this is our sentinel
                    if trimmed.starts_with(SENTINEL) && trimmed.ends_with("_END") {
                        // Extract error code
                        let prefix_len = SENTINEL.len() + 1; // +1 for underscore
                        let suffix_len = 4; // "_END"
                        if trimmed.len() > prefix_len + suffix_len {
                            let code_str = &trimmed[prefix_len..trimmed.len() - suffix_len];
                            if let Ok(code) = code_str.parse::<i32>() {
                                exit_code = code;
                            }
                        }
                        if debug_this {
                            eprintln!("DEBUG: Found sentinel, exit code: {}", exit_code);
                        }
                        break;
                    } else if !trimmed.is_empty() {
                        // Regular output
                        output.push_str(&line);
                    }
                }
                Err(e) => {
                    if debug_this {
                        eprintln!("DEBUG: Read error: {}", e);
                    }
                    return Err(e);
                }
            }
        }

        Ok((output, exit_code))
    }
}

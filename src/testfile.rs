use std::io::{self, Write, BufRead, BufReader};
use std::process::{Command, Stdio, Child, ChildStdin, ChildStdout};

const SENTINEL: &str = "__CMD_DONE__";

struct CmdSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl CmdSession {
    fn start() -> io::Result<Self> {
        // /Q = no echo; /K = keep running; /V:ON = delayed expansion (!ERRORLEVEL!)
        // PROMPT set short to reduce noise.
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

    /// Run a single command in the persistent cmd.exe.
    /// Returns (combined_output, exit_code).
    fn run(&mut self, cmd: &str) -> io::Result<(String, i32)> {
        // We redirect stderr to stdout so we only have to read one stream.
        // We bracket output and then print the sentinel with the current ERRORLEVEL.
        // Note: newlines are important; `\r\n` works fine for cmd.
        let wrapped = format!(
            "{cmd} 2>&1 & echo {sentinel} !ERRORLEVEL!\r\n",
            cmd = cmd,
            sentinel = SENTINEL
        );
        self.stdin.write_all(wrapped.as_bytes())?;
        self.stdin.flush()?;

        // Read until we hit the sentinel line, collecting output.
        let mut output = String::new();
        let mut exit_code: i32 = 0;

        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                // Child ended unexpectedly
                break;
            }

            // Trim typical CRLF endings for matching, but keep original line in output
            let trimmed = line.trim_end_matches(&['\r', '\n'][..]);

            if let Some(rest) = trimmed.strip_prefix(SENTINEL) {
                // Expected format: "__CMD_DONE__ <code>"
                // Allow with/without leading space
                let code_str = rest.trim();
                if !code_str.is_empty() {
                    if let Ok(code) = code_str.parse::<i32>() {
                        exit_code = code;
                    }
                }
                break; // we've reached the end marker for this command
            } else {
                output.push_str(&line);
            }
        }

        Ok((output, exit_code))
    }
}

fn main() -> io::Result<()> {
    let mut session = CmdSession::start()?;

    // Example 1: simple echo
    let (out, code) = session.run("echo Hello from persistent CMD")?;
    println!("exit={code}\n{out}");

    // Example 2: a command that writes to stderr (merged because of 2>&1)
    let (out, code) = session.run("dir /B non_existing_dir")?;
    println!("exit={code}\n{out}");

    // Example 3: set a var and use it later to prove it's the same session
    session.run("set MYVAR=42")?;
    let (out, code) = session.run("echo MYVAR is %MYVAR%")?;
    println!("exit={code}\n{out}");

    // When you're done, tell cmd.exe to exit this session:
    // (If you skip this, the child will be killed when the process ends.)
    let _ = session.run("exit");

    Ok(())
}

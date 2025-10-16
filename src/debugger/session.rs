use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const SENTINEL: &str = "__CMD_DONE__";

pub struct CmdSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl CmdSession {
    pub fn start() -> io::Result<Self> {
        let mut child = Command::new("cmd")
            .args(["/V:ON", "/Q", "/K", "PROMPT $G"]) // /V:ON enables delayed expansion
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().expect("no stdin");
        let stdout = child.stdout.take().expect("no stdout");

        Ok(Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    pub fn run(&mut self, cmd: &str) -> io::Result<(String, i32)> {
        // Use !ERRORLEVEL! for delayed expansion (enabled with /V:ON)
        let wrapped = format!("{cmd} 2>&1 & echo {SENTINEL} !ERRORLEVEL!\r\n");
        self.stdin.write_all(wrapped.as_bytes())?;
        self.stdin.flush()?;

        let mut output = String::new();
        let mut exit_code = 0;

        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
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

// Save this as src/bin/test_session.rs and run with: cargo run --bin test_session

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn main() -> io::Result<()> {
    println!("Starting minimal cmd.exe test...\n");

    // Test 1: Simple echo with immediate response
    println!("Test 1: Basic echo test");
    let mut child = Command::new("cmd")
        .args(["/Q", "/C", "echo Hello World"])// /C executes and exits
        .stdout(Stdio::piped())
        .spawn()?;

    let output = child.wait_with_output()?;
    println!("Output: {}", String::from_utf8_lossy(&output.stdout));

    // Test 2: Interactive cmd with piped I/O
    println!("\nTest 2: Interactive cmd test");
    let mut child = Command::new("cmd")
        .args(["/Q"])// Just quiet mode
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Send a simple command
    println!("Sending: echo TEST");
    stdin.write_all(b"echo TEST\r\n")?;
    stdin.flush()?;

    // Try to read response
    println!("Waiting for response...");
    let mut line = String::new();

    // Read with timeout using a thread
    let handle = std::thread::spawn(move || {
        let mut line = String::new();
        let mut lines = Vec::new();
        for _ in 0..5 {
          
            // Try to read up to 5 lines
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    println!("Received: '{}'", line.trim());
                    lines.push(line.clone());
                }
                Err(e) => {
                    println!("Error reading: {}", e);
                    break;
                }
            }
        }
        lines
});

    // Wait for thread with timeout
std::thread::sleep(std::time::Duration::from_secs(2));

    // Send exit command
    println!("\nSending exit command...");
    stdin.write_all(b"exit\r\n")?;
stdin.flush()?;

    // Clean up
    drop(stdin);
let _ = child.wait();

    println!("\nTest complete!");
 
   Ok(())
}

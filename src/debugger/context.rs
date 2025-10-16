use super::breakpoints::Breakpoints;
use super::{CmdSession, Frame, RunMode};
use crate::parser::LogicalLine;
use std::collections::HashMap;
use std::io;

pub struct DebugContext {
    session: CmdSession,
    pub variables: HashMap<String, String>,
    pub call_stack: Vec<Frame>,
    pub last_exit_code: i32,
    breakpoints: Breakpoints,
    mode: RunMode,
    step_out_target_depth: usize,
}

impl DebugContext {
    pub fn new(session: CmdSession) -> Self {
        Self {
            session,
            variables: HashMap::new(),
            call_stack: Vec::new(),
            last_exit_code: 0,
            breakpoints: Breakpoints::new(),
            mode: RunMode::Continue,
            step_out_target_depth: 0,
        }
    }

    pub fn session_mut(&mut self) -> &mut CmdSession {
        &mut self.session
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: RunMode) {
        self.mode = mode;
    }

    pub fn print_call_stack(&self, logical: &[LogicalLine]) {
        if self.call_stack.is_empty() {
            eprintln!("\n=== Call Stack: <empty - top level> ===");
            return;
        }

        eprintln!("\n=== Call Stack ({} frames) ===", self.call_stack.len());
        for (i, frame) in self.call_stack.iter().enumerate().rev() {
            let return_line = frame.return_pc.saturating_sub(1);
            if return_line < logical.len() {
                let line = &logical[return_line];
                eprintln!(
                    "  #{}: return to logical line {} (phys line {})",
                    i,
                    frame.return_pc,
                    line.phys_start + 1
                );
            } else {
                eprintln!("  #{}: return to logical line {}", i, frame.return_pc);
            }
        }
        eprintln!();
    }

    pub fn print_variables(&self) {
        if self.variables.is_empty() {
            return;
        }
        eprintln!("\n=== Tracked Variables ===");
        let mut vars: Vec<_> = self.variables.iter().collect();
        vars.sort_by_key(|(k, _)| *k);
        for (key, val) in vars {
            eprintln!("  {}={}", key, val);
        }
        eprintln!();
    }

    pub fn track_set_command(&mut self, line: &str) {
        let line_upper = line.to_uppercase();
        if line_upper.starts_with("SET ") {
            let rest = &line[4..].trim();
            if let Some(eq_pos) = rest.find('=') {
                let key = rest[..eq_pos].trim().to_string();
                let val = rest[eq_pos + 1..].trim().to_string();
                self.variables.insert(key, val);
            }
        }
    }

    pub fn add_breakpoint(&mut self, logical_line: usize) {
        self.breakpoints.add(logical_line);
    }

    #[allow(dead_code)]
    pub fn remove_breakpoint(&mut self, logical_line: usize) {
        self.breakpoints.remove(logical_line);
    }

    pub fn should_stop_at(&self, pc: usize) -> bool {
        match self.mode {
            RunMode::Continue => self.breakpoints.contains(pc),
            RunMode::StepOver | RunMode::StepInto => true,
            RunMode::StepOut => self.call_stack.len() <= self.step_out_target_depth,
        }
    }

    pub fn handle_step_command(&mut self, step_type: &str) {
        match step_type {
            "continue" => {
                self.mode = RunMode::Continue;
                eprintln!("▶️  Continuing execution...");
            }
            "next" | "stepOver" => {
                self.mode = RunMode::StepOver;
                eprintln!("⏭️  Step Over");
            }
            "stepIn" | "stepInto" => {
                self.mode = RunMode::StepInto;
                eprintln!("⤵️  Step Into");
            }
            "stepOut" => {
                self.mode = RunMode::StepOut;
                self.step_out_target_depth = self.call_stack.len().saturating_sub(1);
                eprintln!(
                    "⤴️  Step Out (target depth: {})",
                    self.step_out_target_depth
                );
            }
            _ => {
                eprintln!("❓ Unknown step command: {}", step_type);
            }
        }
    }

    pub fn run_command(&mut self, cmd: &str) -> io::Result<(String, i32)> {
        self.session.run(cmd)
    }
}

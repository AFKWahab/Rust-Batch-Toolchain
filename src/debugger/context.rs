use super::breakpoints::Breakpoints;
use super::{CmdSession, Frame, RunMode};
use crate::parser::LogicalLine;
use std::collections::HashMap;
use std::io;

pub struct DebugContext {
    session: CmdSession,
    /// Global variables (when not in a SETLOCAL scope)
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

    /// Handle SETLOCAL command - creates a new variable scope
    pub fn handle_setlocal(&mut self) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.has_setlocal = true;
            eprintln!("📦 SETLOCAL - created new variable scope");
        }
    }

    /// Handle ENDLOCAL command - restores previous variable scope
    pub fn handle_endlocal(&mut self) {
        if let Some(frame) = self.call_stack.last_mut() {
            if frame.has_setlocal {
                frame.locals.clear();
                frame.has_setlocal = false;
                eprintln!("📤 ENDLOCAL - restored previous scope");
            }
        }
    }

    /// Get all variables visible in current scope (merges global + local)
    pub fn get_visible_variables(&self) -> HashMap<String, String> {
        let mut visible = self.variables.clone();

        // Overlay local variables from current frame if SETLOCAL is active
        if let Some(frame) = self.call_stack.last() {
            if frame.has_setlocal {
                visible.extend(frame.locals.clone());
            }
        }

        visible
    }

    /// Get variables for a specific stack frame (for DAP)
    pub fn get_frame_variables(&self, frame_index: usize) -> HashMap<String, String> {
        if frame_index < self.call_stack.len() {
            let frame = &self.call_stack[frame_index];
            if frame.has_setlocal {
                return frame.locals.clone();
            }
        }
        HashMap::new()
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
                let scope_info = if frame.has_setlocal {
                    format!(" [SETLOCAL: {} vars]", frame.locals.len())
                } else {
                    String::new()
                };
                eprintln!(
                    "  #{}: return to logical line {} (phys line {}){}",
                    i,
                    frame.return_pc,
                    line.phys_start + 1,
                    scope_info
                );
            } else {
                eprintln!("  #{}: return to logical line {}", i, frame.return_pc);
            }
        }
        eprintln!();
    }

    pub fn print_variables(&self) {
        let visible = self.get_visible_variables();
        if visible.is_empty() {
            return;
        }
        eprintln!("\n=== Tracked Variables ===");
        let mut vars: Vec<_> = visible.iter().collect();
        vars.sort_by_key(|(k, _)| *k);
        for (key, val) in vars {
            eprintln!("  {}={}", key, val);
        }
        eprintln!();
    }

    // Replace the track_set_command method in debugger/context.rs

    /// Track SET commands - stores in appropriate scope
    pub fn track_set_command(&mut self, line: &str) {
        let l = line.trim_start();
        if !l.to_uppercase().starts_with("SET ") {
            return;
        }

        let mut rest = l[3..].trim_start();

        // Handle /A (arithmetic) - we can't track these accurately without executing
        if rest.to_uppercase().starts_with("/A") {
            // Skip arithmetic operations like SET /A COUNTER+=1
            // We would need to execute the math to know the value
            return;
        }

        // Handle /P (prompt) - skip these as they require user input
        if rest.to_uppercase().starts_with("/P") {
            return;
        }

        // Handle quoted SET "VAR=VAL"
        let rest = rest.trim();
        let rest = if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
            &rest[1..rest.len() - 1]
        } else {
            rest
        };

        if let Some(eq_pos) = rest.find('=') {
            let key = rest[..eq_pos].trim().to_string();
            let val = rest[eq_pos + 1..].trim().to_string();

            // Only track simple assignments (no operators in the key)
            if !key.is_empty()
                && !key.contains('+')
                && !key.contains('-')
                && !key.contains('*')
                && !key.contains('/')
            {
                // Store in local scope if SETLOCAL is active, otherwise global
                if let Some(frame) = self.call_stack.last_mut() {
                    if frame.has_setlocal {
                        frame.locals.insert(key, val);
                        return;
                    }
                }
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

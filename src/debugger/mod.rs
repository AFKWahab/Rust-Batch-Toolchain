mod breakpoints;
mod context;
mod session;
mod stepping;

pub use context::DebugContext;
pub use session::CmdSession;
pub use stepping::RunMode;

use std::collections::HashMap;

/// Represents a single stack frame with its own variable scope
#[derive(Debug, Clone)]
pub struct Frame {
    pub return_pc: usize,
    pub args: Option<Vec<String>>,
    /// Local variables for this frame (created by SETLOCAL)
    pub locals: HashMap<String, String>,
    /// Whether this frame has SETLOCAL active
    pub has_setlocal: bool,
}

impl Frame {
    pub fn new(return_pc: usize, args: Option<Vec<String>>) -> Self {
        Self {
            return_pc,
            args,
            locals: HashMap::new(),
            has_setlocal: false,
        }
    }
}

/// Helper: unwind the current context at EOF.
pub fn leave_context(call_stack: &mut Vec<Frame>) -> Option<usize> {
    if let Some(frame) = call_stack.pop() {
        Some(frame.return_pc)
    } else {
        None
    }
}

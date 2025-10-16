mod breakpoints;
mod context;
mod session;
mod stepping;

pub use context::DebugContext;
pub use session::CmdSession;
pub use stepping::RunMode;

// Re-export Frame for use in executor
pub struct Frame {
    pub return_pc: usize,
    #[allow(dead_code)]
    pub args: Option<Vec<String>>,
    #[allow(dead_code)]
    pub locals: Option<Vec<String>>,
}

/// Helper: unwind the current context at EOF.
pub fn leave_context(call_stack: &mut Vec<Frame>) -> Option<usize> {
    if let Some(frame) = call_stack.pop() {
        Some(frame.return_pc)
    } else {
        None
    }
}

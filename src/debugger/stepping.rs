/// Run modes for the debugger
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunMode {
    Continue,
    StepOver,
    StepInto,
    StepOut,
}

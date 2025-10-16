/// One physical→logical joined line (before block annotation).
#[derive(Debug, Clone)]
pub struct JoinedLine {
    pub text: String,
    pub phys_start: usize,
    pub phys_end: usize,
}

/// Final normalized line with block metadata for the debugger.
#[derive(Debug, Clone)]
pub struct LogicalLine {
    pub text: String,
    pub phys_start: usize,
    pub phys_end: usize,
    pub group_id: Option<u32>,
    pub group_depth: u16,
}

/// Output of preprocessing: logical lines + mapping back to physical indices.
#[derive(Debug)]
pub struct PreprocessResult {
    pub logical: Vec<LogicalLine>,
    pub phys_to_logical: Vec<usize>,
}

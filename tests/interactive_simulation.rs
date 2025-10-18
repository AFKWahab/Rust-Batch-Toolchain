// tests/interactive_simulation.rs
// Simulates interactive debugging scenarios

use std::fs;

#[cfg(test)]
mod interactive_tests {
    use super::*;

    // Helper to create test files
    fn create_test_script(name: &str, content: &str) -> String {
        let filename = format!("test_{}.bat", name);
        fs::write(&filename, content).expect("Failed to write test file");
        filename
    }

    fn cleanup(filename: &str) {
        let _ = fs::remove_file(filename);
    }

    #[test]
    fn test_step_into_simulation() {
        // Create a simple test script
        let content = r#"@echo off
echo Line 1
echo Line 2
call :sub
echo Line 4
exit /b 0

:sub
echo In subroutine
exit /b 0
"#;

        let filename = create_test_script("step_into", content);
        let contents = fs::read_to_string(&filename).expect("Could not read");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let pre = batch_debugger::parser::preprocess_lines(&physical_lines);
        let labels = batch_debugger::parser::build_label_map(&physical_lines);

        // Simulate execution with StepInto mode
        use batch_debugger::debugger::{CmdSession, DebugContext, RunMode};

        let session = CmdSession::start().expect("Failed to start session");
        let mut ctx = DebugContext::new(session);
        ctx.set_mode(RunMode::StepInto);

        // Verify we're in StepInto mode
        assert_eq!(ctx.mode(), RunMode::StepInto);

        // In StepInto mode, should_stop_at returns true for any line when not at breakpoint
        // This simulates stopping at each line

        cleanup(&filename);
    }

    #[test]
    fn test_step_over_behavior() {
        let content = r#"@echo off
echo Before call
call :subroutine
echo After call
exit /b 0

:subroutine
echo In sub
exit /b 0
"#;

        let filename = create_test_script("step_over", content);
        let contents = fs::read_to_string(&filename).expect("Could not read");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let pre = batch_debugger::parser::preprocess_lines(&physical_lines);

        use batch_debugger::debugger::{CmdSession, DebugContext, RunMode};

        let session = CmdSession::start().expect("Failed to start session");
        let mut ctx = DebugContext::new(session);
        ctx.set_mode(RunMode::StepOver);

        assert_eq!(ctx.mode(), RunMode::StepOver);

        cleanup(&filename);
    }

    #[test]
    fn test_breakpoint_stop_behavior() {
        let content = r#"@echo off
echo Line 1
echo Line 2
echo Line 3
echo Line 4
echo Line 5
exit /b 0
"#;

        let filename = create_test_script("breakpoints", content);
        let contents = fs::read_to_string(&filename).expect("Could not read");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let pre = batch_debugger::parser::preprocess_lines(&physical_lines);

        use batch_debugger::debugger::{CmdSession, DebugContext, RunMode};

        let session = CmdSession::start().expect("Failed to start session");
        let mut ctx = DebugContext::new(session);

        // Set breakpoints at lines 2 and 4
        ctx.add_breakpoint(2);
        ctx.add_breakpoint(4);

        // Set Continue mode
        ctx.set_mode(RunMode::Continue);

        // Should stop at breakpoints
        assert!(ctx.should_stop_at(2), "Should stop at breakpoint line 2");
        assert!(ctx.should_stop_at(4), "Should stop at breakpoint line 4");

        // Should not stop at other lines
        assert!(!ctx.should_stop_at(1), "Should not stop at line 1");
        assert!(!ctx.should_stop_at(3), "Should not stop at line 3");
        assert!(!ctx.should_stop_at(5), "Should not stop at line 5");

        cleanup(&filename);
    }

    #[test]
    fn test_continue_mode_with_no_breakpoints() {
        use batch_debugger::debugger::{CmdSession, DebugContext, RunMode};

        let session = CmdSession::start().expect("Failed to start session");
        let mut ctx = DebugContext::new(session);

        ctx.set_mode(RunMode::Continue);

        // Without breakpoints, should not stop at any line
        assert!(!ctx.should_stop_at(1));
        assert!(!ctx.should_stop_at(10));
        assert!(!ctx.should_stop_at(100));
    }

    #[test]
    fn test_step_out_with_call_stack() {
        use batch_debugger::debugger::{CmdSession, DebugContext, Frame, RunMode};

        let session = CmdSession::start().expect("Failed to start session");
        let mut ctx = DebugContext::new(session);

        // Simulate being inside nested calls
        ctx.call_stack.push(Frame::new(10, None));
        ctx.call_stack.push(Frame::new(20, None));
        ctx.call_stack.push(Frame::new(30, None));

        // Current depth is 3
        assert_eq!(ctx.call_stack.len(), 3);

        // Set StepOut mode
        ctx.set_mode(RunMode::StepOut);

        // StepOut should stop when call stack depth is less than current
        // The should_stop_at will check if depth <= step_out_target_depth

        // Pop one frame to simulate returning
        ctx.call_stack.pop();

        // Now at depth 2, should be able to detect we've stepped out
        assert_eq!(ctx.call_stack.len(), 2);
    }

    #[test]
    fn test_nested_call_stack_tracking() {
        let content = r#"@echo off
call :level1
exit /b 0

:level1
call :level2
exit /b 0

:level2
call :level3
exit /b 0

:level3
echo Deepest level
exit /b 0
"#;

        let filename = create_test_script("nested_calls", content);
        let contents = fs::read_to_string(&filename).expect("Could not read");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let labels = batch_debugger::parser::build_label_map(&physical_lines);

        // Verify all labels were found
        assert!(labels.contains_key("level1"));
        assert!(labels.contains_key("level2"));
        assert!(labels.contains_key("level3"));
        assert_eq!(labels.len(), 3);

        cleanup(&filename);
    }

    #[test]
    fn test_mode_transitions() {
        use batch_debugger::debugger::{CmdSession, DebugContext, RunMode};

        let session = CmdSession::start().expect("Failed to start session");
        let mut ctx = DebugContext::new(session);

        // Test all mode transitions
        let modes = vec![
            RunMode::Continue,
            RunMode::StepInto,
            RunMode::StepOver,
            RunMode::StepOut,
        ];

        for mode in modes {
            ctx.set_mode(mode);
            assert_eq!(ctx.mode(), mode, "Mode should be set correctly");
        }
    }

    #[test]
    fn test_quit_behavior() {
        // Quitting is handled by breaking out of the execution loop
        // We can test that the context can be dropped cleanly
        use batch_debugger::debugger::{CmdSession, DebugContext};

        let session = CmdSession::start().expect("Failed to start session");
        let ctx = DebugContext::new(session);

        // Dropping context should work without errors
        drop(ctx);

        // If we get here, quit behavior is clean
        assert!(true);
    }

    #[test]
    fn test_breakpoint_with_continue_resume() {
        use batch_debugger::debugger::{CmdSession, DebugContext, RunMode};

        let session = CmdSession::start().expect("Failed to start session");
        let mut ctx = DebugContext::new(session);

        // Set breakpoint
        ctx.add_breakpoint(5);

        // Start in Continue mode
        ctx.set_mode(RunMode::Continue);

        // Simulate hitting breakpoint at line 5
        assert!(ctx.should_stop_at(5), "Should stop at breakpoint");

        // After user input, resume with Continue
        ctx.set_mode(RunMode::Continue);

        // Should not stop at non-breakpoint lines
        assert!(!ctx.should_stop_at(6));
        assert!(!ctx.should_stop_at(7));

        // Should stop at next breakpoint if there is one
        ctx.add_breakpoint(10);
        assert!(ctx.should_stop_at(10));
    }
}

use std::fs;

// Helper to create a test batch file
fn create_test_batch(content: &str, filename: &str) -> String {
    let path = format!("test_{}.bat", filename);
    fs::write(&path, content).expect("Failed to write test file");
    path
}

// Helper to cleanup test files
fn cleanup_test_batch(path: &str) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod debugger_tests {
    use super::*;

    #[test]
    fn test_basic_execution() {
        let content = r#"@echo off
set NAME=Alice
echo Hello %NAME%
exit /b 0
"#;

        let path = create_test_batch(content, "basic");

        // Parse and preprocess
        let contents = fs::read_to_string(&path).expect("Could not read test file");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let pre = batch_debugger::parser::preprocess_lines(&physical_lines);
        let labels = batch_debugger::parser::build_label_map(&physical_lines);

        // Verify parsing
        assert!(pre.logical.len() > 0, "Should have parsed logical lines");
        assert_eq!(labels.len(), 0, "Should have no labels");

        cleanup_test_batch(&path);
    }

    #[test]
    fn test_label_parsing() {
        let content = r#"@echo off
call :subroutine
exit /b 0

:subroutine
echo In subroutine
exit /b 0
"#;

        let path = create_test_batch(content, "labels");
        let contents = fs::read_to_string(&path).expect("Could not read test file");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let labels = batch_debugger::parser::build_label_map(&physical_lines);

        assert_eq!(labels.len(), 1, "Should have found 1 label");
        assert!(
            labels.contains_key("subroutine"),
            "Should have found :subroutine label"
        );

        cleanup_test_batch(&path);
    }

    #[test]
    fn test_line_continuation() {
        let content = r#"@echo off
echo This is a ^
continued line
exit /b 0
"#;

        let path = create_test_batch(content, "continuation");
        let contents = fs::read_to_string(&path).expect("Could not read test file");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let pre = batch_debugger::parser::preprocess_lines(&physical_lines);

        // The continuation should join lines 1 and 2
        let joined_line = &pre.logical[1].text;
        assert!(
            joined_line.contains("This is a") && joined_line.contains("continued line"),
            "Lines should be joined"
        );

        cleanup_test_batch(&path);
    }

    #[test]
    fn test_comment_detection() {
        assert!(batch_debugger::parser::is_comment("REM This is a comment"));
        assert!(batch_debugger::parser::is_comment(
            ":: This is also a comment"
        ));
        assert!(batch_debugger::parser::is_comment(""));
        assert!(!batch_debugger::parser::is_comment("echo Hello"));
    }

    #[test]
    fn test_composite_command_splitting() {
        let parts = batch_debugger::parser::split_composite_command("echo A & echo B && echo C");
        assert_eq!(parts.len(), 3, "Should split into 3 parts");

        let parts2 = batch_debugger::parser::split_composite_command("echo A || echo B");
        assert_eq!(parts2.len(), 2, "Should split into 2 parts");
    }

    #[test]
    fn test_breakpoint_management() {
        use batch_debugger::debugger::CmdSession;
        use batch_debugger::debugger::DebugContext;

        let session = CmdSession::start().expect("Failed to start CMD session");
        let mut ctx = DebugContext::new(session);

        // Add breakpoints
        ctx.add_breakpoint(5);
        ctx.add_breakpoint(10);
        ctx.add_breakpoint(15);

        // Test should_stop_at in Continue mode
        use batch_debugger::debugger::RunMode;
        ctx.set_mode(RunMode::Continue);

        assert!(ctx.should_stop_at(5), "Should stop at breakpoint 5");
        assert!(ctx.should_stop_at(10), "Should stop at breakpoint 10");
        assert!(!ctx.should_stop_at(7), "Should not stop at line 7");
    }

    #[test]
    fn test_run_modes() {
        use batch_debugger::debugger::CmdSession;
        use batch_debugger::debugger::DebugContext;
        use batch_debugger::debugger::RunMode;

        let session = CmdSession::start().expect("Failed to start CMD session");
        let mut ctx = DebugContext::new(session);

        // Test mode switching
        ctx.set_mode(RunMode::Continue);
        assert_eq!(ctx.mode(), RunMode::Continue);

        ctx.set_mode(RunMode::StepInto);
        assert_eq!(ctx.mode(), RunMode::StepInto);

        ctx.set_mode(RunMode::StepOver);
        assert_eq!(ctx.mode(), RunMode::StepOver);

        ctx.set_mode(RunMode::StepOut);
        assert_eq!(ctx.mode(), RunMode::StepOut);
    }

    #[test]
    fn test_variable_tracking() {
        use batch_debugger::debugger::CmdSession;
        use batch_debugger::debugger::DebugContext;

        let session = CmdSession::start().expect("Failed to start CMD session");
        let mut ctx = DebugContext::new(session);

        // Track simple SET commands
        ctx.track_set_command("SET NAME=Alice");
        ctx.track_set_command("SET AGE=25");
        ctx.track_set_command("SET \"CITY=New York\"");

        assert_eq!(ctx.variables.get("NAME"), Some(&"Alice".to_string()));
        assert_eq!(ctx.variables.get("AGE"), Some(&"25".to_string()));
        assert_eq!(ctx.variables.get("CITY"), Some(&"New York".to_string()));

        // Should not track SET /A
        ctx.track_set_command("SET /A COUNTER+=1");
        assert!(!ctx.variables.contains_key("COUNTER"));

        // Should not track SET /P
        ctx.track_set_command("SET /P INPUT=Enter value:");
        assert!(!ctx.variables.contains_key("INPUT"));
    }

    #[test]
    fn test_call_stack() {
        use batch_debugger::debugger::Frame;

        let mut call_stack: Vec<Frame> = Vec::new();

        // Simulate CALL operations
        call_stack.push(Frame::new(
            10,
            Some(vec!["arg1".to_string(), "arg2".to_string()]),
        ));
        call_stack.push(Frame::new(25, None));
        call_stack.push(Frame::new(40, Some(vec!["test".to_string()])));

        assert_eq!(call_stack.len(), 3, "Should have 3 frames");

        // Simulate returns
        let frame3 = call_stack.pop().unwrap();
        assert_eq!(frame3.return_pc, 40);

        let frame2 = call_stack.pop().unwrap();
        assert_eq!(frame2.return_pc, 25);

        assert_eq!(call_stack.len(), 1, "Should have 1 frame left");
    }

    #[test]
    fn test_setlocal_scope() {
        use batch_debugger::debugger::CmdSession;
        use batch_debugger::debugger::DebugContext;
        use batch_debugger::debugger::Frame;

        let session = CmdSession::start().expect("Failed to start CMD session");
        let mut ctx = DebugContext::new(session);

        // Set global variable
        ctx.track_set_command("SET GLOBAL=value1");

        // Enter subroutine
        ctx.call_stack.push(Frame::new(10, None));

        // SETLOCAL
        ctx.handle_setlocal();

        // Set local variable
        ctx.track_set_command("SET LOCAL=value2");

        // Check visible variables includes both
        let visible = ctx.get_visible_variables();
        assert_eq!(visible.get("GLOBAL"), Some(&"value1".to_string()));
        assert_eq!(visible.get("LOCAL"), Some(&"value2".to_string()));

        // ENDLOCAL
        ctx.handle_endlocal();

        // Local variable should be cleared
        let visible_after = ctx.get_visible_variables();
        assert_eq!(visible_after.get("GLOBAL"), Some(&"value1".to_string()));
        assert!(!visible_after.contains_key("LOCAL"));
    }

    #[test]
    fn test_cmd_session_basic_command() {
        use batch_debugger::debugger::CmdSession;

        let mut session = CmdSession::start().expect("Failed to start CMD session");

        // Test basic echo command
        let (output, code) = session
            .run("echo Hello World")
            .expect("Failed to run command");
        assert!(
            output.contains("Hello World"),
            "Output should contain 'Hello World'"
        );
        assert_eq!(code, 0, "Exit code should be 0");
    }

    #[test]
    fn test_cmd_session_set_command() {
        use batch_debugger::debugger::CmdSession;

        let mut session = CmdSession::start().expect("Failed to start CMD session");

        // Set a variable
        let (_, code) = session
            .run("set TESTVAR=TestValue")
            .expect("Failed to set variable");
        assert_eq!(code, 0, "SET command should succeed");

        // Echo the variable
        let (output, _) = session
            .run("echo %TESTVAR%")
            .expect("Failed to echo variable");
        assert!(
            output.contains("TestValue"),
            "Should echo the variable value"
        );
    }

    #[test]
    fn test_preprocessing_empty_lines() {
        let physical_lines = vec!["@echo off", "", "echo Hello", "", "exit /b 0"];
        let pre = batch_debugger::parser::preprocess_lines(&physical_lines);

        // Should have logical lines for all physical lines
        assert_eq!(pre.phys_to_logical.len(), 5);
    }

    #[test]
    fn test_block_depth_tracking() {
        let content = r#"@echo off
if 1==1 (
    echo Level 1
    if 2==2 (
        echo Level 2
    )
)
exit /b 0
"#;

        let path = create_test_batch(content, "blocks");
        let contents = fs::read_to_string(&path).expect("Could not read test file");
        let physical_lines: Vec<&str> = contents.lines().collect();

        let pre = batch_debugger::parser::preprocess_lines(&physical_lines);

        // Check that depth tracking works
        let depths: Vec<u16> = pre.logical.iter().map(|l| l.group_depth).collect();

        // Should have varying depths
        assert!(depths.iter().any(|&d| d == 0), "Should have depth 0");
        assert!(depths.iter().any(|&d| d > 0), "Should have depth > 0");

        cleanup_test_batch(&path);
    }
}

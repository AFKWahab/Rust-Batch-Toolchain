/// Represents a command operator for composite commands
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandOp {
    Unconditional, // &
    And,           // &&
    Or,            // ||
}

/// A single command part in a composite command line
#[derive(Debug, Clone)]
pub struct CommandPart {
    pub text: String,
    pub op: Option<CommandOp>,
}

/// Normalize whitespace in command
pub fn normalize_whitespace(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split a command line by composite operators (&, &&, ||)
pub fn split_composite_command(line: &str) -> Vec<CommandPart> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        if ch == '^' {
            escaped = true;
            current.push(ch);
            continue;
        }

        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
            continue;
        }

        if !in_quotes && ch == '&' {
            let op = if chars.peek() == Some(&'&') {
                chars.next();
                CommandOp::And
            } else {
                CommandOp::Unconditional
            };

            parts.push(CommandPart {
                text: current.trim().to_string(),
                op: Some(op),
            });
            current.clear();
            continue;
        }

        if !in_quotes && ch == '|' {
            if chars.peek() == Some(&'|') {
                chars.next();
                parts.push(CommandPart {
                    text: current.trim().to_string(),
                    op: Some(CommandOp::Or),
                });
                current.clear();
                continue;
            }
        }

        current.push(ch);
    }

    if !current.trim().is_empty() {
        parts.push(CommandPart {
            text: current.trim().to_string(),
            op: None,
        });
    }

    parts
}

/// Check if line is a comment
pub fn is_comment(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty()
        || trimmed.to_uppercase().starts_with("REM ")
        || trimmed.starts_with("::")
        || trimmed.to_uppercase().starts_with("REM\t")
}

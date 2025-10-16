use super::types::{JoinedLine, LogicalLine, PreprocessResult};

/// Join physical lines that are continued with a trailing caret `^`.
pub fn join_continued_lines(physical: &[&str]) -> Vec<JoinedLine> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < physical.len() {
        let start = i;
        let mut buf = String::new();

        loop {
            let line = physical[i];
            let (continues, trimmed_without_one_caret) = {
                let det = line.trim_end_matches([' ', '\t']);
                let mut caret_count = 0usize;
                for ch in det.chars().rev() {
                    if ch == '^' {
                        caret_count += 1;
                    } else {
                        break;
                    }
                }
                let continues = !det.is_empty()
                    && det.chars().rev().find(|c| !c.is_whitespace()) == Some('^')
                    && (caret_count % 2 == 1);

                if continues {
                    let det_len = det.len();
                    let cut_at = det_len - 1;
                    let (head, _tail_ws) = line.split_at(cut_at);
                    let mut joined = String::with_capacity(head.len() + 1);
                    joined.push_str(head);
                    (true, joined)
                } else {
                    (false, line.to_string())
                }
            };

            if buf.is_empty() {
                buf.push_str(&trimmed_without_one_caret);
            } else {
                buf.push(' ');
                buf.push_str(&trimmed_without_one_caret);
            }

            if continues {
                i += 1;
                if i >= physical.len() {
                    break;
                }
                continue;
            } else {
                break;
            }
        }

        let end = i;
        out.push(JoinedLine {
            text: buf,
            phys_start: start,
            phys_end: end,
        });

        i += 1;
    }

    out
}

/// Annotate joined lines with parenthesis block depth and group_id.
pub fn annotate_blocks(joined: Vec<JoinedLine>) -> Vec<LogicalLine> {
    let mut logical = Vec::with_capacity(joined.len());

    let mut depth: i32 = 0;
    let mut group_id_stack: Vec<u32> = Vec::new();
    let mut next_group_id: u32 = 1;

    for j in joined {
        let line_depth = depth.max(0) as u16;
        let current_group = group_id_stack.last().copied();

        let mut chars = j.text.chars().peekable();
        let mut escaped = false;

        while let Some(ch) = chars.next() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '^' {
                escaped = true;
                continue;
            }
            match ch {
                '(' => {
                    depth += 1;
                    group_id_stack.push(next_group_id);
                    next_group_id += 1;
                }
                ')' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    let _ = group_id_stack.pop();
                }
                _ => {}
            }
        }

        logical.push(LogicalLine {
            text: j.text,
            phys_start: j.phys_start,
            phys_end: j.phys_end,
            group_id: current_group,
            group_depth: line_depth,
        });
    }

    logical
}

/// Full preprocessing pipeline
pub fn preprocess_lines(physical: &[&str]) -> PreprocessResult {
    let joined = join_continued_lines(physical);
    let logical = annotate_blocks(joined.clone());

    let mut phys_to_logical = vec![0usize; physical.len()];
    for (li, j) in joined.iter().enumerate() {
        for p in j.phys_start..=j.phys_end {
            phys_to_logical[p] = li;
        }
    }

    PreprocessResult {
        logical,
        phys_to_logical,
    }
}

use std::collections::HashMap;

/// Scan labels (case-insensitive)
pub fn build_label_map(lines: &[&str]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with(':') && t.len() > 1 {
            let label_text = &t[1..];
            let label_name = label_text.split_whitespace().next().unwrap_or(label_text);
            map.insert(label_name.trim().to_lowercase(), i);
        }
    }
    map
}

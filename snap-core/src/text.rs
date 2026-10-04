//! Input normalization: tabs, common indentation, size caps.

/// Expands tabs, strips common leading whitespace and surrounding blank lines,
/// and caps line/column counts. Returns the lines and whether anything was cut.
pub fn prepare(
    code: &str,
    tab_width: usize,
    max_lines: usize,
    max_columns: usize,
) -> (Vec<String>, bool) {
    let tab_width = tab_width.max(1);
    let mut lines: Vec<String> = code
        .lines()
        .map(|l| expand_tabs(l.trim_end(), tab_width))
        .collect();

    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    let indent = lines
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    if indent > 0 {
        for l in &mut lines {
            if !l.is_empty() {
                l.drain(..indent);
            }
        }
    }

    let mut truncated = false;
    if lines.len() > max_lines.max(1) {
        lines.truncate(max_lines.max(1));
        truncated = true;
    }
    for l in &mut lines {
        if let Some((idx, _)) = l.char_indices().nth(max_columns.max(1)) {
            l.truncate(idx);
            l.push('…');
            truncated = true;
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    (lines, truncated)
}

fn expand_tabs(line: &str, tab_width: usize) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let n = tab_width - col % tab_width;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedents_and_expands() {
        let (l, t) = prepare("\n    if x {\n\t  y\n    }\n\n", 4, 100, 100);
        assert_eq!(l, vec!["if x {", "  y", "}"]);
        assert!(!t);
    }

    #[test]
    fn keeps_blank_lines_inside() {
        let (l, _) = prepare("  a\n\n  b", 4, 100, 100);
        assert_eq!(l, vec!["a", "", "b"]);
    }

    #[test]
    fn truncates_columns() {
        let (l, t) = prepare("abcdef", 4, 100, 3);
        assert_eq!(l, vec!["abc…"]);
        assert!(t);
    }
}

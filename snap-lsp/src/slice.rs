//! Extracting the selected text from a document given an LSP range.

use tower_lsp::lsp_types::{Position, Range};

/// Byte offset of an LSP position (UTF-16 code units) in `text`, clamped to the text.
pub fn offset(text: &str, pos: Position) -> usize {
    let mut line_start = 0;
    for _ in 0..pos.line {
        match text[line_start..].find('\n') {
            Some(i) => line_start += i + 1,
            None => return text.len(),
        }
    }
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |i| line_start + i);
    let line = &text[line_start..line_end];
    let mut units = 0u32;
    for (i, c) in line.char_indices() {
        if units >= pos.character {
            return line_start + i;
        }
        units += c.len_utf16() as u32;
    }
    line_end
}

/// Returns the selected text. When the selection starts mid-line, the part of
/// the first line before the cursor is replaced by spaces so indentation of the
/// following lines stays aligned (it is removed again by dedenting).
pub fn selection(text: &str, range: Range) -> String {
    let start = offset(text, range.start);
    let end = offset(text, range.end).max(start);
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let prefix = &text[line_start..start];
    let pad: String = prefix
        .chars()
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect();
    format!("{pad}{}", &text[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
        Range::new(Position::new(sl, sc), Position::new(el, ec))
    }

    #[test]
    fn slices_lines() {
        let t = "fn a() {\n    x();\n}\n";
        assert_eq!(selection(t, r(1, 0, 2, 1)), "    x();\n}");
        assert_eq!(selection(t, r(1, 4, 1, 8)), "    x();");
    }

    #[test]
    fn handles_utf16() {
        // "😀" is 2 UTF-16 units, 4 bytes.
        let t = "let s = \"😀é\";\nok";
        assert_eq!(
            &t[offset(t, Position::new(0, 11))..offset(t, Position::new(0, 12))],
            "é"
        );
        assert_eq!(offset(t, Position::new(1, 0)), t.find("ok").unwrap());
    }

    #[test]
    fn clamps_out_of_range() {
        let t = "ab\ncd";
        assert_eq!(offset(t, Position::new(9, 0)), t.len());
        assert_eq!(offset(t, Position::new(0, 99)), 2);
    }

    #[test]
    fn crlf() {
        let t = "a\r\nbc\r\n";
        assert_eq!(selection(t, r(1, 0, 1, 2)), "bc");
    }
}

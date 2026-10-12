//! Conversions between textarea offsets (UTF-16 code units, as the DOM counts) and lines/columns.

/// 1-based (line, column) of a UTF-16 offset.
pub fn line_col_at(text: &str, utf16_offset: u32) -> (u32, u32) {
    let mut line = 1;
    let mut column = 1;
    let mut consumed = 0_u32;
    for character in text.chars() {
        if consumed >= utf16_offset {
            break;
        }
        consumed += u32::try_from(character.len_utf16()).unwrap_or(1);
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// UTF-16 range (start, end without the newline) of a 1-based line; clamps past the end.
pub fn line_range(text: &str, line: u32) -> (u32, u32) {
    let mut current = 1;
    let mut offset = 0_u32;
    let mut start = 0_u32;
    for character in text.chars() {
        if character == '\n' {
            if current == line {
                return (start, offset);
            }
            current += 1;
            start = offset + 1;
        }
        offset += u32::try_from(character.len_utf16()).unwrap_or(1);
    }
    (start, offset)
}

/// Leading whitespace of the line that contains the UTF-16 offset, plus one indent step when the
/// line opens a block (`key:` or `- key:`), for Enter auto-indent.
pub fn indent_for_newline(text: &str, utf16_offset: u32) -> String {
    let (line, _) = line_col_at(text, utf16_offset);
    let (start, _) = line_range(text, line);
    let line_text: String = utf16_slice(text, start, utf16_offset);
    let indent: String = line_text.chars().take_while(|c| *c == ' ').collect();
    let opens_block = line_text.trim_end().ends_with(':') && !line_text.trim_start().starts_with('#');
    let item_indent = if line_text.trim_start().starts_with("- ") { "  " } else { "" };
    if opens_block { format!("{indent}{item_indent}  ") } else { format!("{indent}{item_indent}") }
}

fn utf16_slice(text: &str, start: u32, end: u32) -> String {
    let mut offset = 0_u32;
    let mut slice = String::new();
    for character in text.chars() {
        if offset >= end {
            break;
        }
        if offset >= start {
            slice.push(character);
        }
        offset += u32::try_from(character.len_utf16()).unwrap_or(1);
    }
    slice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_utf16() {
        let text = "ab\nçé🙂x\n";
        assert_eq!(line_col_at(text, 0), (1, 1));
        assert_eq!(line_col_at(text, 3), (2, 1));
        // 🙂 is two UTF-16 units but one column.
        assert_eq!(line_col_at(text, 7), (2, 4));
    }

    #[test]
    fn line_range_finds_lines() {
        let text = "one\ntwo\nthree";
        assert_eq!(line_range(text, 1), (0, 3));
        assert_eq!(line_range(text, 2), (4, 7));
        assert_eq!(line_range(text, 3), (8, 13));
        assert_eq!(line_range(text, 9), (8, 13));
    }

    #[test]
    fn newline_indent_follows_yaml_blocks() {
        assert_eq!(indent_for_newline("nodes:", 6), "  ");
        assert_eq!(indent_for_newline("  - id: a", 9), "    ");
        assert_eq!(indent_for_newline("    icon: server", 16), "    ");
        assert_eq!(indent_for_newline("  # note:", 9), "  ");
    }
}

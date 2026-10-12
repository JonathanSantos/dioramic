//! Pin for YAML specs: line-level edits, so comments and formatting survive byte for byte. Only
//! the lines holding the values that change are touched; [`super::verify`] checks the result.

use super::PinnedValue;

/// Top-level keys that make up the header of a spec, after which a missing section is inserted.
const HEADER_KEYS: [&str; 6] = ["diorama", "title", "subtitle", "theme", "layout", "view"];

/// Indentation used when a block section gets its first child.
const DEFAULT_INDENT: &str = "  ";

/// Writes `values` into the YAML text.
pub fn pin(text: &str, values: &[PinnedValue]) -> String {
    let mut document = Document::parse(text);
    for pinned in values {
        let value = pinned.yaml_scalar();
        match pinned.section {
            Some(section) => document.set_nested(section, pinned.key, &value),
            None => document.set_top_level(pinned.key, &value),
        }
    }
    document.render()
}

/// The spec as lines, remembering the line ending and the trailing newline.
struct Document {
    lines: Vec<String>,
    newline: &'static str,
    trailing_newline: bool,
}

impl Document {
    fn parse(text: &str) -> Self {
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_newline = text.is_empty() || text.ends_with('\n');
        let body = text.strip_suffix(newline).unwrap_or(text);
        let lines = if body.is_empty() { Vec::new() } else { body.split(newline).map(str::to_owned).collect() };
        Self { lines, newline, trailing_newline }
    }

    fn render(&self) -> String {
        let mut text = self.lines.join(self.newline);
        if self.trailing_newline {
            text.push_str(self.newline);
        }
        text
    }

    /// Index of the top-level `key:` line.
    fn top_level(&self, key: &str) -> Option<usize> {
        self.lines.iter().position(|line| top_level_key(line) == Some(key))
    }

    fn set_top_level(&mut self, key: &str, value: &str) {
        match self.top_level(key) {
            Some(index) => self.lines[index] = replace_value(&self.lines[index], value),
            None => self.insert_header_line(format!("{key}: {value}")),
        }
    }

    fn set_nested(&mut self, section: &str, key: &str, value: &str) {
        let Some(index) = self.top_level(section) else {
            self.insert_header_line(format!("{section}: {{ {key}: {value} }}"));
            return;
        };
        let (content, comment) = split_comment(&self.lines[index]);
        let inline = content.split_once(':').map(|(_, rest)| rest.trim()).unwrap_or_default();
        let (properties, _) = split_node_properties(inline);
        let body = section_body(content);
        let prefix = if properties.is_empty() { format!("{section}:") } else { format!("{section}: {properties}") };
        if body.is_empty() {
            self.set_in_block(index, key, value);
        } else if body.starts_with('{') {
            self.set_in_flow_section(index, &prefix, key, value);
        } else {
            // `view: null`, an alias…: replace with a fresh flow map.
            self.lines[index] = format!("{prefix} {{ {key}: {value} }}{comment}");
        }
    }

    /// Edits a flow-map section (`view: { … }`). A map split over several lines is folded into one
    /// line, unless comments live inside it (folding would swallow them): then nothing is edited
    /// and the semantic check refuses the pin.
    fn set_in_flow_section(&mut self, index: usize, prefix: &str, key: &str, value: &str) {
        let Some(last) = self.flow_end(index) else { return };
        let inner_comment = self.lines[index..last].iter().any(|line| !split_comment(line).1.is_empty());
        if inner_comment {
            return;
        }
        let comment = split_comment(&self.lines[last]).1.to_owned();
        let mut flow = String::new();
        for (offset, line) in self.lines[index..=last].iter().enumerate() {
            let content = split_comment(line).0;
            let part = if offset == 0 { section_body(content) } else { content.trim() };
            if !part.is_empty() {
                if !flow.is_empty() {
                    flow.push(' ');
                }
                flow.push_str(part);
            }
        }
        if !flow.ends_with('}') {
            return;
        }
        let edited = format!("{prefix} {}{comment}", set_in_flow_map(&flow, key, value));
        self.lines.splice(index..=last, [edited]);
    }

    /// Line where the flow collection opened on line `start` closes (brackets balanced, outside quotes).
    fn flow_end(&self, start: usize) -> Option<usize> {
        let mut depth = 0_i32;
        for (index, line) in self.lines.iter().enumerate().skip(start) {
            depth += bracket_balance(split_comment(line).0);
            if depth <= 0 {
                return Some(index);
            }
        }
        None
    }

    fn set_in_block(&mut self, section_index: usize, key: &str, value: &str) {
        let children = self.block_children(section_index);
        let mut child_indent: Option<String> = None;
        for index in children {
            let line = &self.lines[index];
            let indent = leading_whitespace(line);
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let indent_owned = indent.to_owned();
            let is_direct_child = child_indent.get_or_insert_with(|| indent_owned.clone()) == &indent_owned;
            if is_direct_child && mapping_key(line.trim_start()) == Some(key) {
                self.lines[index] = replace_value(line, value);
                self.drop_nested_block(index);
                return;
            }
        }
        let indent = child_indent.unwrap_or_else(|| DEFAULT_INDENT.to_owned());
        self.lines.insert(section_index + 1, format!("{indent}{key}: {value}"));
    }

    /// A child that held a nested block (`grid:` followed by more indented lines) now holds its value
    /// on one line: the old block lines go. Blocks with comments stay, so the semantic check refuses the
    /// pin instead of dropping a comment.
    fn drop_nested_block(&mut self, child_index: usize) {
        let indent = leading_whitespace(&self.lines[child_index]).len();
        let start = child_index + 1;
        let end = self.lines[start..]
            .iter()
            .position(|line| !line.trim().is_empty() && leading_whitespace(line).len() <= indent)
            .map_or(self.lines.len(), |offset| start + offset);
        let block = &self.lines[start..end];
        let has_comment =
            block.iter().any(|line| !split_comment(line).1.is_empty() || line.trim_start().starts_with('#'));
        let has_content = block.iter().any(|line| !line.trim().is_empty());
        if has_content && !has_comment {
            let trailing_blank = block.iter().rev().take_while(|line| line.trim().is_empty()).count();
            self.lines.drain(start..end - trailing_blank);
        }
    }

    /// Indices of the lines belonging to the block under a top-level key.
    fn block_children(&self, section_index: usize) -> std::ops::Range<usize> {
        let start = section_index + 1;
        let end = self.lines[start..]
            .iter()
            .position(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#') && leading_whitespace(line).is_empty()
            })
            .map_or(self.lines.len(), |offset| start + offset);
        start..end
    }

    /// Inserts a top-level line after the last header key (or at the top when there is none).
    fn insert_header_line(&mut self, line: String) {
        let after = self
            .lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| top_level_key(line).is_some_and(|key| HEADER_KEYS.contains(&key)))
            .map(|(index, _)| index);
        let position = match after {
            Some(index) => self.block_children(index).end,
            None => 0,
        };
        // Keep blank lines that separated the header from the next section below the new line.
        let position = self.skip_back_blank_lines(position.min(self.lines.len()));
        self.lines.insert(position, line);
    }

    fn skip_back_blank_lines(&self, mut position: usize) -> usize {
        while position > 0 && self.lines[position - 1].trim().is_empty() {
            position -= 1;
        }
        position
    }
}

/// Splits YAML node properties (`&anchor`, `!tag`) off the start of an inline value.
fn split_node_properties(inline: &str) -> (&str, &str) {
    let mut rest = inline.trim_start();
    while rest.starts_with(['&', '!']) {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        rest = rest[end..].trim_start();
    }
    (inline[..inline.len() - rest.len()].trim(), rest)
}

/// Value of a `section: &anchor value` line without its node properties.
fn section_body(content: &str) -> &str {
    let inline = content.split_once(':').map(|(_, rest)| rest.trim()).unwrap_or_default();
    split_node_properties(inline).1
}

/// Opened minus closed `{`/`[` on a line, ignoring brackets inside quotes.
fn bracket_balance(content: &str) -> i32 {
    let mut balance = 0;
    let mut quote: Option<char> = None;
    for character in content.chars() {
        match (quote, character) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(character),
            (None, '{' | '[') => balance += 1,
            (None, '}' | ']') => balance -= 1,
            _ => {}
        }
    }
    balance
}

/// Key of a top-level mapping line (`view: …` → `view`); `None` for indented, list or comment lines.
fn top_level_key(line: &str) -> Option<&str> {
    if !leading_whitespace(line).is_empty() {
        return None;
    }
    mapping_key(line)
}

/// Key of a `key: value` / `key:` line (already trimmed at the start).
fn mapping_key(line: &str) -> Option<&str> {
    if line.starts_with('#') || line.starts_with('-') {
        return None;
    }
    let (key, rest) = line.split_once(':')?;
    let is_identifier = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    (is_identifier && (rest.is_empty() || rest.starts_with([' ', '\t']))).then_some(key)
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Splits `content  # comment` into (`content`, `  # comment`), ignoring `#` inside quotes.
fn split_comment(line: &str) -> (&str, &str) {
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (index, character) in line.char_indices() {
        match (quote, character) {
            (Some(open), c) if c == open => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, '#') if previous.is_whitespace() => {
                let content = line[..index].trim_end();
                return (content, &line[content.len()..]);
            }
            _ => {}
        }
        previous = character;
    }
    (line, "")
}

/// Replaces the value of a `key: value  # comment` line, keeping indentation and comment.
fn replace_value(line: &str, value: &str) -> String {
    let (content, comment) = split_comment(line);
    let key_end = content.find(':').map_or(content.len(), |colon| colon + 1);
    format!("{} {value}{comment}", &content[..key_end])
}

/// Sets `key` inside a one-line flow map `{ a: 1, key: 2 }`, keeping the other entries verbatim.
fn set_in_flow_map(flow: &str, key: &str, value: &str) -> String {
    let inner = flow.trim_start_matches('{').trim_end_matches('}').trim();
    let mut entries: Vec<String> =
        split_top_level_commas(inner).into_iter().map(str::trim).map(str::to_owned).collect();
    entries.retain(|entry| !entry.is_empty());
    match entries.iter_mut().find(|entry| mapping_key(entry) == Some(key)) {
        Some(entry) => *entry = format!("{key}: {value}"),
        None => entries.push(format!("{key}: {value}")),
    }
    format!("{{ {} }}", entries.join(", "))
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (index, character) in text.char_indices() {
        match (quote, character) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(character),
            (None, '{' | '[') => depth += 1,
            (None, '}' | ']') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pin::pin_view;
    use dioramic_desktop_ipc::{DiagramTheme, ViewDensity, ViewRotation, ViewState};

    fn rotation(rotation: ViewRotation) -> ViewState {
        ViewState { rotation: Some(rotation), ..ViewState::default() }
    }

    #[test]
    fn empty_view_changes_nothing() {
        let text = "# c\ndiorama: 1\nnodes: []\n";
        assert_eq!(pin_view(text, &ViewState::default()), text);
    }

    #[test]
    fn updates_a_flow_map_in_place() {
        let text = "diorama: 1\nview: { rotation: 0, padding: 2 } # camera\nnodes: []\n";
        let pinned = pin_view(text, &rotation(ViewRotation::Deg90));
        assert_eq!(pinned, "diorama: 1\nview: { rotation: 90, padding: 2 } # camera\nnodes: []\n");
    }

    #[test]
    fn adds_a_key_to_a_flow_map() {
        let text = "diorama: 1\nlayout: { wrap: 2 }\n";
        let pinned = pin_view(text, &ViewState { density: Some(ViewDensity::Compact), ..ViewState::default() });
        assert_eq!(pinned, "diorama: 1\nlayout: { wrap: 2, density: compact }\n");
    }

    #[test]
    fn updates_a_block_map_keeping_comments() {
        let text = "diorama: 1\nview:\n    # chosen by hand\n    rotation: 180   # keep\n    title: x\nnodes: []\n";
        let pinned = pin_view(text, &rotation(ViewRotation::Auto));
        assert_eq!(
            pinned,
            "diorama: 1\nview:\n    # chosen by hand\n    rotation: auto   # keep\n    title: x\nnodes: []\n"
        );
    }

    #[test]
    fn inserts_into_a_block_map_with_its_indent() {
        let text = "diorama: 1\nlayout:\n    wrap: 2\nnodes: []\n";
        let pinned = pin_view(text, &ViewState { density: Some(ViewDensity::Spaced), ..ViewState::default() });
        assert_eq!(pinned, "diorama: 1\nlayout:\n    density: spaced\n    wrap: 2\nnodes: []\n");
    }

    #[test]
    fn ignores_nested_keys_with_the_same_name() {
        let text = "view:\n  inner:\n    rotation: 1\n";
        let pinned = pin_view(text, &rotation(ViewRotation::Deg270));
        assert_eq!(pinned, "view:\n  rotation: 270\n  inner:\n    rotation: 1\n");
    }

    #[test]
    fn creates_missing_sections_after_the_header() {
        let text = "# Checkout\ndiorama: 1\ntitle: Checkout\n\nnodes:\n  - { id: a }\n";
        let view = ViewState {
            rotation: Some(ViewRotation::Deg90),
            density: Some(ViewDensity::Compact),
            theme: Some(DiagramTheme::Midnight),
            ..ViewState::default()
        };
        let pinned = pin_view(text, &view);
        assert_eq!(
            pinned,
            "# Checkout\ndiorama: 1\ntitle: Checkout\nview: { rotation: 90 }\nlayout: { density: compact }\ntheme: midnight\n\nnodes:\n  - { id: a }\n"
        );
    }

    #[test]
    fn replaces_theme_keeping_comment_and_crlf() {
        let text = "diorama: 1\r\ntheme: blueprint # light\r\n";
        let pinned = pin_view(text, &ViewState { theme: Some(DiagramTheme::Paper), ..ViewState::default() });
        assert_eq!(pinned, "diorama: 1\r\ntheme: paper # light\r\n");
    }

    #[test]
    fn hash_inside_quotes_is_not_a_comment() {
        assert_eq!(split_comment("title: \"API #1\" # c"), ("title: \"API #1\"", " # c"));
        assert_eq!(split_comment("a: b#c"), ("a: b#c", ""));
    }

    #[test]
    fn handles_text_without_trailing_newline() {
        let pinned = pin_view("diorama: 1", &rotation(ViewRotation::Deg0));
        assert_eq!(pinned, "diorama: 1\nview: { rotation: 0 }");
    }
}

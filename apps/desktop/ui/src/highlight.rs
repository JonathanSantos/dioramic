//! Tiny YAML highlighter tuned for Diorama specs: keys, scalars, comments, flow punctuation, and
//! the spec's own vocabulary (edge arrows `->`/`<->`/`..`, style tags `@primary`).
//! Line based and forgiving: it colours, it never validates.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use dioramic_desktop_ipc::Severity;

/// Class of a highlighted piece of text (the CSS class is `t-<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Whitespace and anything unclassified.
    Plain,
    /// Mapping key.
    Key,
    /// Plain scalar value.
    Value,
    /// Quoted string.
    String,
    /// Number.
    Number,
    /// `true`, `false`, `null`, `~`, `auto`.
    Literal,
    /// `# comment`.
    Comment,
    /// `{ } [ ] , :`.
    Punct,
    /// List dash.
    Dash,
    /// Edge operator (`->`, `<->`, `..`).
    Arrow,
    /// Edge style tag (`@primary`).
    Style,
}

impl TokenKind {
    fn class(self) -> &'static str {
        match self {
            Self::Plain => "t-plain",
            Self::Key => "t-key",
            Self::Value => "t-value",
            Self::String => "t-string",
            Self::Number => "t-number",
            Self::Literal => "t-literal",
            Self::Comment => "t-comment",
            Self::Punct => "t-punct",
            Self::Dash => "t-dash",
            Self::Arrow => "t-arrow",
            Self::Style => "t-style",
        }
    }
}

/// A highlighted slice of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    /// Class.
    pub kind: TokenKind,
    /// Text (borrowed from the line).
    pub text: &'a str,
}

/// Splits one line into tokens whose texts concatenate back to the line.
pub fn tokenize_line(line: &str) -> Vec<Token<'_>> {
    Lexer { line, position: 0, flow_depth: 0, tokens: Vec::new() }.run()
}

struct Lexer<'a> {
    line: &'a str,
    position: usize,
    flow_depth: u32,
    tokens: Vec<Token<'a>>,
}

impl<'a> Lexer<'a> {
    fn run(mut self) -> Vec<Token<'a>> {
        while let Some(character) = self.peek() {
            let start = self.position;
            match character {
                ' ' | '\t' => self.take_while(TokenKind::Plain, |c| c == ' ' || c == '\t'),
                '#' if self.at_comment_start() => self.take_rest(TokenKind::Comment),
                '"' | '\'' => self.take_string(character),
                '-' if self.is_list_dash() => self.take_chars(TokenKind::Dash, 1),
                '{' | '[' => {
                    self.flow_depth += 1;
                    self.take_chars(TokenKind::Punct, 1);
                }
                '}' | ']' => {
                    self.flow_depth = self.flow_depth.saturating_sub(1);
                    self.take_chars(TokenKind::Punct, 1);
                }
                ',' if self.flow_depth > 0 => self.take_chars(TokenKind::Punct, 1),
                ':' if self.is_separator_colon(start) => self.take_chars(TokenKind::Punct, 1),
                _ => self.take_scalar(),
            }
            if self.position == start {
                // Never loop forever on an unexpected character.
                self.take_chars(TokenKind::Plain, 1);
            }
        }
        self.tokens
    }

    fn peek(&self) -> Option<char> {
        self.line[self.position..].chars().next()
    }

    fn push(&mut self, kind: TokenKind, end: usize) {
        if end > self.position {
            self.tokens.push(Token { kind, text: &self.line[self.position..end] });
            self.position = end;
        }
    }

    fn take_chars(&mut self, kind: TokenKind, count: usize) {
        let end = self.line[self.position..]
            .char_indices()
            .nth(count)
            .map_or(self.line.len(), |(offset, _)| self.position + offset);
        self.push(kind, end);
    }

    fn take_while(&mut self, kind: TokenKind, keep: impl Fn(char) -> bool) {
        let end = self.line[self.position..]
            .find(|c: char| !keep(c))
            .map_or(self.line.len(), |offset| self.position + offset);
        self.push(kind, end);
    }

    fn take_rest(&mut self, kind: TokenKind) {
        self.push(kind, self.line.len());
    }

    fn at_comment_start(&self) -> bool {
        self.line[..self.position].chars().last().is_none_or(char::is_whitespace)
    }

    fn is_list_dash(&self) -> bool {
        let before_is_indent = self.line[..self.position].chars().all(|c| c == ' ' || c == '\t');
        let after = self.line[self.position + 1..].chars().next();
        before_is_indent && after.is_none_or(|c| c == ' ')
    }

    fn is_separator_colon(&self, at: usize) -> bool {
        self.line[at + 1..]
            .chars()
            .next()
            .is_none_or(|c| c == ' ' || c == '\t' || (self.flow_depth > 0 && (c == ',' || c == '}')))
    }

    fn take_string(&mut self, quote: char) {
        let body_start = self.position + 1;
        let mut escaped = false;
        let mut end = self.line.len();
        for (offset, character) in self.line[body_start..].char_indices() {
            if quote == '"' && character == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if character == quote && !escaped {
                end = body_start + offset + character.len_utf8();
                break;
            }
            escaped = false;
        }
        let is_key = self.followed_by_separator(end);
        self.push(if is_key { TokenKind::Key } else { TokenKind::String }, end);
    }

    /// A plain scalar ends at a separator colon, a comment, or flow punctuation inside `{}`/`[]`.
    fn take_scalar(&mut self) {
        let end = self.scalar_end();
        let text = &self.line[self.position..end];
        let trimmed_len = text.trim_end().len();
        let is_key = self.followed_by_separator(end);
        self.push_scalar_words(self.position + trimmed_len, is_key);
        self.push(TokenKind::Plain, end);
    }

    fn scalar_end(&self) -> usize {
        let mut previous = ' ';
        for (offset, character) in self.line[self.position..].char_indices() {
            let index = self.position + offset;
            let ends = match character {
                ':' => offset > 0 && self.is_separator_colon(index),
                '#' => previous.is_whitespace() && offset > 0,
                ',' | '{' | '}' | '[' | ']' => self.flow_depth > 0,
                _ => false,
            };
            if ends {
                return index;
            }
            previous = character;
        }
        self.line.len()
    }

    fn followed_by_separator(&self, end: usize) -> bool {
        let rest = self.line[end..].trim_start_matches([' ', '\t']);
        rest.starts_with(':') && self.is_separator_colon(self.line.len() - rest.len())
    }

    /// Splits a scalar on spaces so edge shorthands (`api -> db: SQL @secondary`) get their own colours.
    fn push_scalar_words(&mut self, end: usize, is_key: bool) {
        let scalar = &self.line[self.position..end];
        let base = self.position;
        let mut cursor = 0;
        for word in scalar.split(' ') {
            let word_start = base + cursor;
            let word_end = word_start + word.len();
            if !word.is_empty() {
                self.push(word_kind(word, is_key), word_end);
            }
            cursor += word.len() + 1;
            let space_end = (base + cursor).min(end);
            self.push(TokenKind::Plain, space_end);
        }
    }
}

fn word_kind(word: &str, is_key: bool) -> TokenKind {
    let is_arrow = word.len() >= 2 && word.chars().all(|c| matches!(c, '-' | '<' | '>' | '.' | '='));
    if is_arrow {
        TokenKind::Arrow
    } else if word.starts_with('@') && word.len() > 1 {
        TokenKind::Style
    } else if is_key {
        TokenKind::Key
    } else if word.parse::<f64>().is_ok() {
        TokenKind::Number
    } else if matches!(word, "true" | "false" | "null" | "~" | "auto" | "yes" | "no") {
        TokenKind::Literal
    } else {
        TokenKind::Value
    }
}

/// Marker drawn on a line: the worst diagnostic on it and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMark {
    /// Severity (decides the colour).
    pub severity: Severity,
    /// Message of the lens.
    pub message: String,
    /// Field the diagnostic is about (`from`, `icon`); only that part of the line is underlined.
    pub focus: Option<String>,
    /// Other diagnostics on the same line (shown as "+N").
    pub others: usize,
}

/// Fields of an edge shorthand (`a -> b: label @style`).
const EDGE_FIELDS: [&str; 3] = ["from", "to", "label"];

/// Highlighted HTML of the whole text: one `.row` per line. Marked lines get the severity class,
/// a wavy underline on the offending field and an empty `.eol` marker where the text ends, which the
/// editor measures to place the lens.
pub fn render_html(text: &str, marks: &BTreeMap<u32, LineMark>) -> String {
    let mut html = String::with_capacity(text.len() * 3);
    for (index, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mark = u32::try_from(index + 1).ok().and_then(|number| marks.get(&number));
        let row_class = mark.map_or("row", |mark| match mark.severity {
            Severity::Error => "row has-error",
            Severity::Warning => "row has-warning",
            Severity::Hint => "row has-hint",
        });
        let _ = write!(html, "<div class=\"{row_class}\">");
        let tokens = tokenize_line(line);
        let underlined = mark.map(|mark| underlined_tokens(&tokens, mark.focus.as_deref()));
        for (position, token) in tokens.iter().enumerate() {
            let squiggle = underlined.as_ref().is_some_and(|flags| flags[position]);
            let class = token.kind.class();
            let extra = if squiggle { " squiggle" } else { "" };
            let _ = write!(html, "<span class=\"{class}{extra}\">{}</span>", escape(token.text));
        }
        if mark.is_some() {
            html.push_str("<span class=\"eol\"></span>");
        }
        html.push_str("</div>");
    }
    html
}

fn is_word(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Key
            | TokenKind::Value
            | TokenKind::String
            | TokenKind::Number
            | TokenKind::Literal
            | TokenKind::Style
    )
}

/// Which tokens of a marked line carry the wavy underline: the value of the focused key, or the
/// focused part of an edge shorthand; the whole line's words when the field cannot be found.
pub fn underlined_tokens(tokens: &[Token<'_>], focus: Option<&str>) -> Vec<bool> {
    let picked = focus.and_then(|focus| {
        let arrow = tokens.iter().position(|token| token.kind == TokenKind::Arrow);
        match arrow {
            Some(arrow) if EDGE_FIELDS.contains(&focus) => edge_field(tokens, arrow, focus),
            _ => key_value(tokens, focus),
        }
    });
    picked.unwrap_or_else(|| tokens.iter().map(|token| is_word(token.kind)).collect())
}

/// `from` = words before the arrow, `to` = words after it up to `:`, `label` = words after `:`.
fn edge_field(tokens: &[Token<'_>], arrow: usize, field: &str) -> Option<Vec<bool>> {
    let colon = tokens[arrow..]
        .iter()
        .position(|token| token.kind == TokenKind::Punct && token.text == ":")
        .map_or(tokens.len(), |offset| arrow + offset);
    let range = match field {
        "from" => 0..arrow,
        "to" => arrow + 1..colon,
        _ => colon.saturating_add(1).min(tokens.len())..tokens.len(),
    };
    let flags: Vec<bool> = tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            range.contains(&index) && is_word(token.kind) && (field != "label" || token.kind != TokenKind::Style)
        })
        .collect();
    flags.contains(&true).then_some(flags)
}

/// The scalar words after `key:` up to the next flow separator or comment; the key itself when the
/// value lives on other lines.
fn key_value(tokens: &[Token<'_>], key: &str) -> Option<Vec<bool>> {
    let key_at =
        tokens.iter().position(|token| token.kind == TokenKind::Key && token.text.trim_matches(['"', '\'']) == key)?;
    let mut flags = vec![false; tokens.len()];
    for (index, token) in tokens.iter().enumerate().skip(key_at + 1) {
        let ends = token.kind == TokenKind::Comment
            || (token.kind == TokenKind::Punct && matches!(token.text, "," | "}" | "]"));
        if ends {
            break;
        }
        flags[index] = is_word(token.kind);
    }
    if !flags.contains(&true) {
        flags[key_at] = true;
    }
    Some(flags)
}

/// Escapes text for HTML.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(TokenKind, &str)> {
        tokenize_line(line)
            .into_iter()
            .filter(|token| token.kind != TokenKind::Plain)
            .map(|token| (token.kind, token.text))
            .collect()
    }

    #[test]
    fn tokens_cover_the_whole_line() {
        for line in [
            "  - { id: api, label: \"API #1\", zone: app } # x",
            "edges:",
            "  - users -> lb: HTTPS @primary",
            "",
            "a: 'it''s'",
        ] {
            let joined: String = tokenize_line(line).iter().map(|token| token.text).collect();
            assert_eq!(joined, line);
        }
    }

    #[test]
    fn key_value_and_comment() {
        assert_eq!(
            kinds("title: Checkout Service # name"),
            vec![
                (TokenKind::Key, "title"),
                (TokenKind::Punct, ":"),
                (TokenKind::Value, "Checkout"),
                (TokenKind::Value, "Service"),
                (TokenKind::Comment, "# name"),
            ]
        );
    }

    #[test]
    fn flow_map_with_quoted_hash() {
        assert_eq!(
            kinds("- { id: api1, label: \"API #1\" }"),
            vec![
                (TokenKind::Dash, "-"),
                (TokenKind::Punct, "{"),
                (TokenKind::Key, "id"),
                (TokenKind::Punct, ":"),
                (TokenKind::Value, "api1"),
                (TokenKind::Punct, ","),
                (TokenKind::Key, "label"),
                (TokenKind::Punct, ":"),
                (TokenKind::String, "\"API #1\""),
                (TokenKind::Punct, "}"),
            ]
        );
    }

    #[test]
    fn edge_shorthand_gets_arrows_and_styles() {
        assert_eq!(
            kinds("  - api2 -> cache: GET / SET @secondary"),
            vec![
                (TokenKind::Dash, "-"),
                (TokenKind::Key, "api2"),
                (TokenKind::Arrow, "->"),
                (TokenKind::Key, "cache"),
                (TokenKind::Punct, ":"),
                (TokenKind::Value, "GET"),
                (TokenKind::Value, "/"),
                (TokenKind::Value, "SET"),
                (TokenKind::Style, "@secondary"),
            ]
        );
    }

    #[test]
    fn numbers_and_literals() {
        assert_eq!(
            kinds("view: { rotation: 90, auto: true }"),
            vec![
                (TokenKind::Key, "view"),
                (TokenKind::Punct, ":"),
                (TokenKind::Punct, "{"),
                (TokenKind::Key, "rotation"),
                (TokenKind::Punct, ":"),
                (TokenKind::Number, "90"),
                (TokenKind::Punct, ","),
                (TokenKind::Key, "auto"),
                (TokenKind::Punct, ":"),
                (TokenKind::Literal, "true"),
                (TokenKind::Punct, "}"),
            ]
        );
    }

    #[test]
    fn urls_are_not_split_on_colons() {
        assert_eq!(
            kinds("link: https://x.io"),
            vec![(TokenKind::Key, "link"), (TokenKind::Punct, ":"), (TokenKind::Value, "https://x.io")]
        );
    }

    #[test]
    fn html_escapes_and_marks_lines() {
        let mut marks = BTreeMap::new();
        marks.insert(
            2,
            LineMark { severity: Severity::Error, message: "bad <ref>".into(), focus: Some("c".into()), others: 0 },
        );
        let html = render_html("a: <b>\nc: d", &marks);
        assert!(html.contains("&lt;b&gt;"));
        assert!(html.contains("row has-error"));
        assert!(html.contains("<span class=\"t-value squiggle\">d</span>"));
        assert!(html.contains("<span class=\"eol\"></span>"));
        assert_eq!(html.matches("<div class=").count(), 2);
    }

    fn squiggled<'a>(line: &'a str, focus: Option<&str>) -> Vec<&'a str> {
        let tokens = tokenize_line(line);
        let flags = underlined_tokens(&tokens, focus);
        tokens.iter().zip(flags).filter(|(_, on)| *on).map(|(token, _)| token.text).collect()
    }

    #[test]
    fn edge_shorthand_underlines_only_the_focused_end() {
        let line = "  - api9 -> db: SQL @secondary";
        assert_eq!(squiggled(line, Some("from")), vec!["api9"]);
        assert_eq!(squiggled(line, Some("to")), vec!["db"]);
        assert_eq!(squiggled(line, Some("label")), vec!["SQL"]);
    }

    #[test]
    fn flow_map_underlines_the_value_of_the_key() {
        let line = "  - { id: lb, icon: loadbalancr, label: Load Balancer }";
        assert_eq!(squiggled(line, Some("icon")), vec!["loadbalancr"]);
        assert_eq!(squiggled(line, Some("label")), vec!["Load", "Balancer"]);
    }

    #[test]
    fn block_key_without_value_underlines_the_key() {
        assert_eq!(squiggled("view:", Some("view")), vec!["view"]);
    }

    #[test]
    fn unknown_focus_underlines_the_whole_line() {
        assert_eq!(squiggled("  - a -> b", None), vec!["a", "b"]);
        assert_eq!(squiggled("title: x", Some("missing")), vec!["title", "x"]);
    }
}

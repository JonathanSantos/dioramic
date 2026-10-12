//! Finds the editor line a diagnostic path (`nodes[3].icon`) points at, for diagnostics the engine
//! reports without a source position. Indentation based: good enough for hand and AI written specs.

use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

fn parse_path(path: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    for part in path.split('.').filter(|part| !part.is_empty()) {
        let (key, mut rest) = part.split_once('[').map_or((part, ""), |(key, rest)| (key, rest));
        if !key.is_empty() {
            segments.push(Segment::Key(key));
        }
        while let Some((index, after)) = rest.split_once(']') {
            if let Ok(index) = index.parse() {
                segments.push(Segment::Index(index));
            }
            rest = after.strip_prefix('[').unwrap_or(after);
        }
    }
    segments
}

struct Lines<'a> {
    lines: Vec<&'a str>,
}

impl<'a> Lines<'a> {
    fn is_content(&self, index: usize) -> bool {
        let trimmed = self.lines[index].trim();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    }

    fn indent(&self, index: usize) -> usize {
        let line = self.lines[index];
        line.len() - line.trim_start().len()
    }

    /// Lines indented deeper than `parent`, up to the next sibling or parent.
    fn children(&self, parent: usize) -> Range<usize> {
        let parent_indent = self.indent(parent);
        let start = parent + 1;
        let end = (start..self.lines.len())
            .find(|&index| self.is_content(index) && self.indent(index) <= parent_indent)
            .unwrap_or(self.lines.len());
        start..end
    }

    fn min_indent(&self, scope: &Range<usize>) -> Option<usize> {
        scope.clone().filter(|&index| self.is_content(index)).map(|index| self.indent(index)).min()
    }

    /// `key` of a line (`  - key: v` and `key: v` both yield `key`).
    fn key_of(&self, index: usize) -> Option<&'a str> {
        let content = self.lines[index].trim_start();
        let content = content.strip_prefix("- ").unwrap_or(content).trim_start();
        let (key, _) = content.split_once(':')?;
        let key = key.trim().trim_matches(['"', '\'']);
        (!key.is_empty() && !key.contains(' ')).then_some(key)
    }

    fn is_item(&self, index: usize) -> bool {
        let content = self.lines[index].trim_start();
        content == "-" || content.starts_with("- ")
    }
}

/// 1-based line for a diagnostic path; the deepest segment that could be found wins.
pub fn locate(text: &str, path: &str) -> Option<u32> {
    let lines = Lines { lines: text.lines().collect() };
    let mut scope = 0..lines.lines.len();
    let mut current: Option<usize> = None;
    for segment in parse_path(path) {
        let found = match segment {
            Segment::Key(key) => find_key(&lines, current, &scope, key),
            Segment::Index(index) => find_item(&lines, &scope, index),
        };
        match found {
            Some(line) => {
                current = Some(line);
                scope = lines.children(line);
            }
            None => break,
        }
    }
    current.and_then(|line| u32::try_from(line + 1).ok())
}

/// Key the diagnostic is about (`edges[3].from` → `from`), so the editor underlines only that
/// field; `None` when the path ends on a whole item (`nodes[2]`) or is empty.
pub fn focus_key(path: &str) -> Option<String> {
    match parse_path(path).last()? {
        Segment::Key(key) => Some((*key).to_owned()),
        Segment::Index(_) => None,
    }
}

fn find_key(lines: &Lines<'_>, current: Option<usize>, scope: &Range<usize>, key: &str) -> Option<usize> {
    // The item line itself carries its first key (`- id: api`), or a whole flow map.
    if let Some(item) = current.filter(|&line| lines.is_item(line)) {
        if lines.key_of(item) == Some(key) {
            return Some(item);
        }
        if scope.is_empty() && lines.lines[item].contains(&format!("{key}:")) {
            return Some(item);
        }
    }
    let indent = lines.min_indent(scope)?;
    scope
        .clone()
        .filter(|&line| lines.is_content(line) && lines.indent(line) == indent && !lines.is_item(line))
        .find(|&line| lines.key_of(line) == Some(key))
        .or_else(|| current.filter(|&line| lines.lines[line].contains(&format!("{key}:"))))
}

fn find_item(lines: &Lines<'_>, scope: &Range<usize>, wanted: usize) -> Option<usize> {
    let indent = lines.min_indent(scope)?;
    scope
        .clone()
        .filter(|&line| lines.is_content(line) && lines.indent(line) == indent && lines.is_item(line))
        .nth(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOW: &str = "diorama: 1\n# nodes\nnodes:\n  - { id: users, icon: cloud }\n  - { id: lb, icon: loadbalancer }\nedges:\n  - users -> lb\n";
    const BLOCK: &str = "nodes:\n  - id: a\n    icon: server\n  - id: b\n    # comment\n    icon: databse\n    note: x\nview:\n  rotation: 90\n";

    #[test]
    fn parses_paths() {
        assert_eq!(parse_path("nodes[3].icon"), vec![Segment::Key("nodes"), Segment::Index(3), Segment::Key("icon")]);
        assert_eq!(parse_path("a[1][2]"), vec![Segment::Key("a"), Segment::Index(1), Segment::Index(2)]);
        assert!(parse_path("").is_empty());
    }

    #[test]
    fn finds_flow_items() {
        assert_eq!(locate(FLOW, "nodes[1].icon"), Some(5));
        assert_eq!(locate(FLOW, "nodes[0]"), Some(4));
        assert_eq!(locate(FLOW, "edges[0]"), Some(7));
    }

    #[test]
    fn finds_block_items_and_nested_keys() {
        assert_eq!(locate(BLOCK, "nodes[1].icon"), Some(6));
        assert_eq!(locate(BLOCK, "nodes[1].id"), Some(4));
        assert_eq!(locate(BLOCK, "view.rotation"), Some(9));
    }

    #[test]
    fn focus_is_the_last_key_of_the_path() {
        assert_eq!(focus_key("edges[3].from").as_deref(), Some("from"));
        assert_eq!(focus_key("nodes[1].icon").as_deref(), Some("icon"));
        assert_eq!(focus_key("view.rotation").as_deref(), Some("rotation"));
        assert_eq!(focus_key("nodes[2]"), None);
        assert_eq!(focus_key(""), None);
    }

    #[test]
    fn falls_back_to_the_deepest_match() {
        assert_eq!(locate(BLOCK, "nodes[7].icon"), Some(1));
        assert_eq!(locate(FLOW, "styles.x"), None);
        assert_eq!(locate(FLOW, ""), None);
    }
}

//! Edge shorthand: `<from> <op> <to> [: label] [@style]` (`docs/04-spec.md#edge`).
//!
//! Examples: `"fw -> core: 1Gbps trunk @trunk"`, `"rack .. core: Comms room rack"`, `"lb -> api1 @primary"`.
//! The YAML single-key form `{ "fw -> core": "1Gbps trunk @trunk" }` is handled by normalize, which
//! joins key and value as `"<key>: <value>"` and calls [`parse_edge_shorthand`].

use thiserror::Error;

use crate::common::ArrowEnds;

/// Edge operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeOp {
    /// `->`: arrow at the target.
    Forward,
    /// `<-`: arrow at the source.
    Backward,
    /// `<->`: arrows at both ends.
    Both,
    /// `--`: plain line.
    Line,
    /// `..>`: dashed with arrow (async / logical dependency).
    DashedForward,
    /// `..`: dashed, no arrow.
    Dashed,
}

impl EdgeOp {
    /// All operators, longest token first (the order a tokenizer must try them).
    pub const ALL: [EdgeOp; 6] =
        [Self::Both, Self::DashedForward, Self::Forward, Self::Backward, Self::Line, Self::Dashed];

    /// Operator text.
    pub fn token(self) -> &'static str {
        match self {
            Self::Forward => "->",
            Self::Backward => "<-",
            Self::Both => "<->",
            Self::Line => "--",
            Self::DashedForward => "..>",
            Self::Dashed => "..",
        }
    }

    /// Arrow ends implied by the operator.
    pub fn arrow(self) -> ArrowEnds {
        match self {
            Self::Forward | Self::DashedForward => ArrowEnds::End,
            Self::Backward => ArrowEnds::Start,
            Self::Both => ArrowEnds::Both,
            Self::Line | Self::Dashed => ArrowEnds::None,
        }
    }

    /// True for `..>` and `..`.
    pub fn is_dashed(self) -> bool {
        matches!(self, Self::DashedForward | Self::Dashed)
    }
}

/// Parsed shorthand. `from`/`to` are kept as written (direction is NOT swapped for `<-`;
/// the arrow goes to the source instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeShorthand {
    /// Left node id.
    pub from: String,
    /// Operator.
    pub op: EdgeOp,
    /// Right node id.
    pub to: String,
    /// Label after `:` (trimmed; None when absent or empty).
    pub label: Option<String>,
    /// Style after `@` (None when absent).
    pub style: Option<String>,
}

/// Why a shorthand string could not be parsed. Messages are English, shown to the AI.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ShorthandError {
    /// No operator found.
    #[error("missing edge operator; expected one of ->, <-, <->, --, ..>, ..")]
    MissingOperator,
    /// Empty node id on one side.
    #[error("missing node id {side} the operator")]
    MissingNode {
        /// `"before"` or `"after"`.
        side: &'static str,
    },
    /// `@` with nothing after it, or style with spaces.
    #[error("invalid style '{0}'; write @name with no spaces")]
    BadStyle(String),
}

/// Parses `"<from> <op> <to> [: label] [@style]"`.
///
/// Rules: the style marker is the last ` @word` of the string; the label is everything after the first `:`
/// that follows the target id; ids are trimmed; spaces around the operator are optional (`a->b`).
pub fn parse_edge_shorthand(text: &str) -> Result<EdgeShorthand, ShorthandError> {
    let (body, style) = split_style(text.trim())?;
    let (head, label) = split_label(body);
    let (from, op, to) = split_operator(head)?;
    Ok(EdgeShorthand { from: from.to_owned(), op, to: to.to_owned(), label, style })
}

/// Marker that starts the style suffix.
const STYLE_MARKER: char = '@';
/// Separator between the endpoints and the label.
const LABEL_SEPARATOR: char = ':';

/// Splits off the trailing `@style`. Only an `@` at the start or after whitespace counts, so labels such as
/// `ops@example.com` stay intact.
fn split_style(text: &str) -> Result<(&str, Option<String>), ShorthandError> {
    let Some(marker) = find_style_marker(text) else {
        return Ok((text, None));
    };
    let style = &text[marker + STYLE_MARKER.len_utf8()..];
    if style.is_empty() || style.chars().any(char::is_whitespace) {
        return Err(ShorthandError::BadStyle(style.trim().to_owned()));
    }
    Ok((text[..marker].trim_end(), Some(style.to_owned())))
}

fn find_style_marker(text: &str) -> Option<usize> {
    text.char_indices()
        .rev()
        .find(|&(index, character)| {
            character == STYLE_MARKER && text[..index].chars().next_back().is_none_or(char::is_whitespace)
        })
        .map(|(index, _)| index)
}

/// Splits at the first `:`; ids never contain `:`, so everything after it is the label.
fn split_label(text: &str) -> (&str, Option<String>) {
    match text.split_once(LABEL_SEPARATOR) {
        Some((head, label)) => {
            let label = label.trim();
            (head, (!label.is_empty()).then(|| label.to_owned()))
        }
        None => (text, None),
    }
}

fn split_operator(head: &str) -> Result<(&str, EdgeOp, &str), ShorthandError> {
    let (start, op) = find_spaced_operator(head)
        .or_else(|| find_operator(head, |_, _| true))
        .ok_or(ShorthandError::MissingOperator)?;
    let from = head[..start].trim();
    let to = head[start + op.token().len()..].trim();
    if from.is_empty() {
        return Err(ShorthandError::MissingNode { side: "before" });
    }
    if to.is_empty() {
        return Err(ShorthandError::MissingNode { side: "after" });
    }
    Ok((from, op, to))
}

/// Prefers an operator surrounded by whitespace, so ids containing `-` (`my-node -> db`) are not split
/// on the wrong token.
fn find_spaced_operator(head: &str) -> Option<(usize, EdgeOp)> {
    find_operator(head, |start, op| {
        let before = head[..start].chars().next_back();
        let after = head[start + op.token().len()..].chars().next();
        before.is_some_and(char::is_whitespace) && after.is_some_and(char::is_whitespace)
    })
}

/// Leftmost operator accepted by `accept`; at each position the longest token wins (`<->` over `<-`).
fn find_operator(head: &str, accept: impl Fn(usize, EdgeOp) -> bool) -> Option<(usize, EdgeOp)> {
    head.char_indices().find_map(|(start, _)| {
        EdgeOp::ALL
            .into_iter()
            .find(|op| head[start..].starts_with(op.token()) && accept(start, *op))
            .map(|op| (start, op))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(from: &str, op: EdgeOp, to: &str, label: Option<&str>, style: Option<&str>) -> EdgeShorthand {
        EdgeShorthand {
            from: from.to_owned(),
            op,
            to: to.to_owned(),
            label: label.map(str::to_owned),
            style: style.map(str::to_owned),
        }
    }

    #[test]
    fn table_of_valid_shorthands() {
        use EdgeOp::*;
        let cases: &[(&str, EdgeShorthand)] = &[
            ("a -> b", edge("a", Forward, "b", None, None)),
            ("a <- b", edge("a", Backward, "b", None, None)),
            ("a <-> b", edge("a", Both, "b", None, None)),
            ("a -- b", edge("a", Line, "b", None, None)),
            ("a ..> b", edge("a", DashedForward, "b", None, None)),
            ("a .. b", edge("a", Dashed, "b", None, None)),
            ("a->b", edge("a", Forward, "b", None, None)),
            ("a<-b", edge("a", Backward, "b", None, None)),
            ("a<->b", edge("a", Both, "b", None, None)),
            ("a--b", edge("a", Line, "b", None, None)),
            ("a..>b", edge("a", DashedForward, "b", None, None)),
            ("a..b", edge("a", Dashed, "b", None, None)),
            ("  fw   ->   core  ", edge("fw", Forward, "core", None, None)),
            ("users -> lb: HTTPS", edge("users", Forward, "lb", Some("HTTPS"), None)),
            ("fw -> core: 1Gbps trunk @trunk", edge("fw", Forward, "core", Some("1Gbps trunk"), Some("trunk"))),
            ("lb -> api1 @primary", edge("lb", Forward, "api1", None, Some("primary"))),
            ("a->b:x@y", edge("a", Forward, "b", Some("x@y"), None)),
            ("a -> b: GET / SET @data", edge("a", Forward, "b", Some("GET / SET"), Some("data"))),
            ("a -> b: http://x:8080/path", edge("a", Forward, "b", Some("http://x:8080/path"), None)),
            ("a -> b: API #1", edge("a", Forward, "b", Some("API #1"), None)),
            ("a -> b: mail ops@example.com", edge("a", Forward, "b", Some("mail ops@example.com"), None)),
            ("a -> b:", edge("a", Forward, "b", None, None)),
            ("a -> b:   @s", edge("a", Forward, "b", None, Some("s"))),
            ("rack .. core: Comms room rack", edge("rack", Dashed, "core", Some("Comms room rack"), None)),
            ("my-node -> db", edge("my-node", Forward, "db", None, None)),
            ("a-b -- c-d", edge("a-b", Line, "c-d", None, None)),
            ("k8s/pod ..> k8s/svc: x -> y", edge("k8s/pod", DashedForward, "k8s/svc", Some("x -> y"), None)),
            ("a <- b: reply @async", edge("a", Backward, "b", Some("reply"), Some("async"))),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_edge_shorthand(text).as_ref(), Ok(expected), "{text}");
        }
    }

    #[test]
    fn table_of_invalid_shorthands() {
        let cases: &[(&str, ShorthandError)] = &[
            ("a b", ShorthandError::MissingOperator),
            ("", ShorthandError::MissingOperator),
            ("a: -> b", ShorthandError::MissingOperator),
            ("-> b", ShorthandError::MissingNode { side: "before" }),
            ("a ->", ShorthandError::MissingNode { side: "after" }),
            ("a -> : label", ShorthandError::MissingNode { side: "after" }),
            ("a -> b @", ShorthandError::BadStyle(String::new())),
            ("a -> b @my style", ShorthandError::BadStyle("my style".to_owned())),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_edge_shorthand(text).as_ref(), Err(expected), "{text}");
        }
    }

    #[test]
    fn operators_map_to_arrows_and_dashes() {
        assert_eq!(EdgeOp::Backward.arrow(), ArrowEnds::Start);
        assert_eq!(EdgeOp::Both.arrow(), ArrowEnds::Both);
        assert_eq!(EdgeOp::Dashed.arrow(), ArrowEnds::None);
        assert!(EdgeOp::DashedForward.is_dashed());
        assert!(!EdgeOp::Line.is_dashed());
    }

    mod properties {
        use proptest::prelude::*;

        use super::super::*;

        fn op() -> impl Strategy<Value = EdgeOp> {
            proptest::sample::select(EdgeOp::ALL.to_vec())
        }

        proptest! {
            #[test]
            fn shorthand_round_trips(
                from in "[a-z][a-z0-9_]{0,12}",
                to in "[a-z][a-z0-9_/-]{0,12}",
                op in op(),
                label in "[A-Za-z0-9#/:.()+][A-Za-z0-9 #/:.()+-]{0,20}[A-Za-z0-9#/.()+]",
                style in "[a-z][a-z0-9_]{0,10}",
            ) {
                let text = format!("{from} {} {to}: {label} @{style}", op.token());
                let parsed = parse_edge_shorthand(&text);
                prop_assert_eq!(parsed, Ok(EdgeShorthand { from, op, to, label: Some(label), style: Some(style) }));
            }
        }
    }
}

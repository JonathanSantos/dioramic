//! Idempotent merge of the `dioramic` server entry into MCP config files (JSON or JSONC).
//!
//! Rules (`research/clients.md` §4.4): only `<root>.dioramic` is ever touched; comments, formatting and
//! every other key are preserved by editing a concrete syntax tree instead of re-serializing the file.

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use serde_json::{Map, Value};

/// Name of the server entry.
pub const SERVER_NAME: &str = "dioramic";

/// Argument list of the server command.
const SERVER_ARGS: [&str; 1] = ["mcp"];

/// Transport assumed when an entry has no `type`.
const DEFAULT_TRANSPORT: &str = "stdio";

/// The entry to write and where it goes. Fields keep their insertion order (`type`, `command`, `args`,
/// `env`) so the written JSON reads naturally.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerEntry {
    /// `mcpServers` (Claude) or `servers` (VS Code).
    pub root_key: &'static str,
    fields: Vec<(&'static str, Value)>,
}

impl ServerEntry {
    /// `{"type": "stdio", "command": …, "args": ["mcp"]}` (+ `"env": {}` when asked, as `claude mcp add` writes).
    pub fn stdio(root_key: &'static str, command: &str, with_type: bool, with_env: bool) -> Self {
        let mut fields = Vec::new();
        if with_type {
            fields.push(("type", Value::from(DEFAULT_TRANSPORT)));
        }
        fields.push(("command", Value::from(command)));
        fields.push(("args", Value::from(SERVER_ARGS.to_vec())));
        if with_env {
            fields.push(("env", Value::Object(Map::new())));
        }
        Self { root_key, fields }
    }

    /// The entry as a JSON value (for comparisons and display).
    pub fn value(&self) -> Value {
        Value::Object(self.fields.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect())
    }

    /// The `command` field.
    pub fn command(&self) -> &str {
        self.fields.iter().find(|(key, _)| *key == "command").and_then(|(_, value)| value.as_str()).unwrap_or_default()
    }

    fn input(&self) -> CstInputValue {
        CstInputValue::Object(self.fields.iter().map(|(key, value)| ((*key).to_owned(), to_input(value))).collect())
    }

    /// Whole minimal file for this entry (2-space indent, final newline).
    pub fn new_file(&self) -> String {
        let text = match CstRootNode::parse("{}", &ParseOptions::default()) {
            Ok(root) => {
                root.object_value_or_set()
                    .append(self.root_key, CstInputValue::Object(vec![(SERVER_NAME.into(), self.input())]));
                root.to_string()
            }
            Err(_) => String::from("{}"),
        };
        format!("{}\n", text.trim_end())
    }

    /// `mcpServers.dioramic`.
    pub fn key_path(&self) -> String {
        format!("{}.{SERVER_NAME}", self.root_key)
    }
}

/// What the merge decided.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeOutcome {
    /// File absent or empty: write this whole text.
    Create(String),
    /// Entry added to an existing file.
    Insert(String),
    /// Existing different entry replaced (`--force`).
    Replace {
        /// New text.
        text: String,
        /// The entry that was there.
        previous: Value,
    },
    /// Entry already present and equivalent.
    Unchanged,
    /// Another entry already runs `dioramic mcp` (no duplicate added).
    Alias(String),
    /// Entry present but different, and no `--force`.
    Conflict {
        /// Current entry.
        current: Value,
    },
}

/// Why a file cannot be merged.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeError {
    /// Not JSON/JSONC.
    Invalid(String),
    /// Valid JSON, unexpected shape.
    Shape(String),
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "not valid JSON: {reason}"),
            Self::Shape(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for MergeError {}

/// Merges `entry` into `existing` (the file text, `None` when the file does not exist).
pub fn merge(existing: Option<&str>, entry: &ServerEntry, force: bool) -> Result<MergeOutcome, MergeError> {
    let Some(text) = existing.filter(|text| !text.trim().is_empty()) else {
        return Ok(MergeOutcome::Create(entry.new_file()));
    };
    let root =
        CstRootNode::parse(text, &ParseOptions::default()).map_err(|error| MergeError::Invalid(error.to_string()))?;
    let root_object =
        root.object_value().ok_or_else(|| MergeError::Shape("the top level is not a JSON object".into()))?;
    let Some(servers_prop) = root_object.get(entry.root_key) else {
        root_object.append(entry.root_key, CstInputValue::Object(vec![(SERVER_NAME.into(), entry.input())]));
        return Ok(MergeOutcome::Insert(root.to_string()));
    };
    let servers = servers_prop
        .object_value()
        .ok_or_else(|| MergeError::Shape(format!("`{}` is not a JSON object", entry.root_key)))?;
    merge_into_servers(&root, &servers, entry, force)
}

fn merge_into_servers(
    root: &CstRootNode,
    servers: &CstObject,
    entry: &ServerEntry,
    force: bool,
) -> Result<MergeOutcome, MergeError> {
    let Some(current_prop) = servers.get(SERVER_NAME) else {
        if let Some(alias) = find_alias(servers, &entry.value()) {
            return Ok(MergeOutcome::Alias(alias));
        }
        servers.append(SERVER_NAME, entry.input());
        return Ok(MergeOutcome::Insert(root.to_string()));
    };
    let current = current_prop.value().and_then(|node| node.to_serde_value()).unwrap_or(Value::Null);
    if equivalent(&current, &entry.value()) {
        return Ok(MergeOutcome::Unchanged);
    }
    if !force {
        return Ok(MergeOutcome::Conflict { current });
    }
    current_prop.set_value(entry.input());
    Ok(MergeOutcome::Replace { text: root.to_string(), previous: current })
}

/// Another server entry that already runs `dioramic mcp` (rule 5: never duplicate).
fn find_alias(servers: &CstObject, wanted: &Value) -> Option<String> {
    servers.properties().into_iter().find_map(|prop| {
        let name = prop.decoded_name()?;
        let value = prop.value()?.to_serde_value()?;
        runs_dioramic(&value, wanted).then_some(name)
    })
}

fn runs_dioramic(value: &Value, wanted: &Value) -> bool {
    let command = value.get("command").and_then(Value::as_str).unwrap_or_default();
    let same_command = Some(command) == wanted.get("command").and_then(Value::as_str) || is_dioramic_binary(command);
    same_command && value.get("args") == wanted.get("args")
}

fn is_dioramic_binary(command: &str) -> bool {
    let file_name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let stem = file_name.strip_suffix(".exe").unwrap_or(file_name);
    stem == SERVER_NAME
}

/// Semantic equality on what matters to the client: `type` (absent = stdio), `command` and `args`.
/// Extra fields the user added (`env`, `timeout`) do not make an entry different.
pub fn equivalent(current: &Value, wanted: &Value) -> bool {
    let transport = |value: &Value| value.get("type").and_then(Value::as_str).unwrap_or(DEFAULT_TRANSPORT).to_owned();
    current.is_object()
        && transport(current) == transport(wanted)
        && current.get("command") == wanted.get("command")
        && current.get("args") == wanted.get("args")
}

fn to_input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(flag) => CstInputValue::Bool(*flag),
        Value::Number(number) => CstInputValue::Number(number.to_string()),
        Value::String(text) => CstInputValue::String(text.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(to_input).collect()),
        Value::Object(fields) => {
            CstInputValue::Object(fields.iter().map(|(key, field)| (key.clone(), to_input(field))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_entry() -> ServerEntry {
        ServerEntry::stdio("mcpServers", "dioramic", true, false)
    }

    fn outcome_text(outcome: MergeOutcome) -> String {
        match outcome {
            MergeOutcome::Create(text) | MergeOutcome::Insert(text) | MergeOutcome::Replace { text, .. } => text,
            other => panic!("expected a write, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_gets_the_minimal_content() {
        let text = outcome_text(merge(None, &claude_entry(), false).unwrap());
        assert_eq!(
            text,
            "{\n  \"mcpServers\": {\n    \"dioramic\": {\n      \"type\": \"stdio\",\n      \"command\": \"dioramic\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}\n"
        );
        assert!(matches!(merge(Some(" \n"), &claude_entry(), false), Ok(MergeOutcome::Create(_))));
    }

    #[test]
    fn merging_twice_is_a_no_op() {
        let first = outcome_text(merge(None, &claude_entry(), false).unwrap());
        assert_eq!(merge(Some(&first), &claude_entry(), false), Ok(MergeOutcome::Unchanged));
    }

    #[test]
    fn other_servers_and_keys_are_preserved() {
        let existing = "{\n  \"mcpServers\": {\n    \"github\": { \"command\": \"gh-mcp\" }\n  },\n  \"other\": 1\n}\n";
        let text = outcome_text(merge(Some(existing), &claude_entry(), false).unwrap());
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["mcpServers"]["github"]["command"], "gh-mcp");
        assert_eq!(value["mcpServers"]["dioramic"]["args"][0], "mcp");
        assert_eq!(value["other"], 1);
        assert!(text.starts_with("{\n  \"mcpServers\": {\n    \"github\": { \"command\": \"gh-mcp\" },"), "{text}");
        assert_eq!(merge(Some(&text), &claude_entry(), false), Ok(MergeOutcome::Unchanged));
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_survive() {
        let entry = ServerEntry::stdio("servers", "dioramic", true, false);
        let existing = "// workspace servers\n{\n  /* inputs first */\n  \"inputs\": [],\n  \"servers\": {\n    // the GitHub one\n    \"github\": { \"type\": \"http\", \"url\": \"https://example.test\" },\n  },\n}\n";
        let text = outcome_text(merge(Some(existing), &entry, false).unwrap());
        for kept in ["// workspace servers", "/* inputs first */", "// the GitHub one", "\"inputs\": []"] {
            assert!(text.contains(kept), "{kept} lost in:\n{text}");
        }
        assert!(text.contains("\"dioramic\""), "{text}");
        assert_eq!(merge(Some(&text), &entry, false), Ok(MergeOutcome::Unchanged));
    }

    #[test]
    fn root_key_is_added_when_missing() {
        let text = outcome_text(merge(Some("{\n  \"numStartups\": 3\n}\n"), &claude_entry(), false).unwrap());
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["numStartups"], 3);
        assert_eq!(value["mcpServers"]["dioramic"]["command"], "dioramic");
    }

    #[test]
    fn different_entry_is_a_conflict_unless_forced() {
        let existing =
            "{ \"mcpServers\": { \"dioramic\": { \"command\": \"/opt/old/dioramic\", \"args\": [\"serve\"] } } }";
        assert!(matches!(merge(Some(existing), &claude_entry(), false), Ok(MergeOutcome::Conflict { .. })));
        let outcome = merge(Some(existing), &claude_entry(), true).unwrap();
        let MergeOutcome::Replace { text, previous } = outcome else { panic!("expected replace") };
        assert_eq!(previous["command"], "/opt/old/dioramic");
        assert_eq!(merge(Some(&text), &claude_entry(), false), Ok(MergeOutcome::Unchanged));
    }

    #[test]
    fn extra_user_fields_do_not_count_as_a_difference() {
        let existing =
            r#"{"mcpServers":{"dioramic":{"command":"dioramic","args":["mcp"],"env":{"X":"1"},"timeout":5000}}}"#;
        assert_eq!(merge(Some(existing), &claude_entry(), false), Ok(MergeOutcome::Unchanged));
    }

    #[test]
    fn an_alias_is_not_duplicated() {
        let existing = r#"{"mcpServers":{"iso":{"command":"/usr/local/bin/dioramic","args":["mcp"]}}}"#;
        assert_eq!(merge(Some(existing), &claude_entry(), false), Ok(MergeOutcome::Alias("iso".into())));
    }

    #[test]
    fn invalid_files_are_refused() {
        assert!(matches!(merge(Some("{ \"mcpServers\": "), &claude_entry(), false), Err(MergeError::Invalid(_))));
        assert!(matches!(merge(Some("[1, 2]"), &claude_entry(), false), Err(MergeError::Shape(_))));
        assert!(matches!(merge(Some("{\"mcpServers\": 3}"), &claude_entry(), false), Err(MergeError::Shape(_))));
    }
}

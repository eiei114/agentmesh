//! Pure escape-sensitive JS/TS edit checks ported from
//! `escape_sensitive_edits.py`.
//!
//! The crate classifies shell commands and scans source text that the caller
//! already holds. It never opens files (`validate_js_ts_file` and its read stay
//! in the vault wrapper) and has no shell, filesystem, network, Multica or git
//! side effects. The Python regexes are matched by hand so the crate needs no
//! regex dependency while keeping Python's case-insensitive and Unicode rules.

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};

/// Suffixes (lowercase, with the dot) treated as JS/TS source.
pub const JS_TS_SUFFIXES: &[&str] = &[".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"];

/// Shell one-liner red flags: Python regex source and reason, in Python order.
pub const UNSAFE_SHELL_EDIT_PATTERNS: &[(&str, &str)] = &[
    (r"python3?\s+-c\b", "python -c one-liner edit"),
    (r"node\s+-e\b", "node -e one-liner edit"),
    (r"\bsed\s+-i\b", "sed -i in-place replacement"),
    (r"\bperl\s+-pi\b", "perl -pi in-place replacement"),
    (r"\bawk\s+-i\s+inplace\b", "awk in-place replacement"),
    (r"powershell\s+-c\b", "powershell -c one-liner edit"),
    (r"pwsh\s+-c\b", "pwsh -c one-liner edit"),
];

/// `kind` of the issue for an assignment split across lines.
pub const KIND_SPLIT_STRING_ASSIGNMENT: &str = "split_string_assignment";
/// `kind` of the issue for a newline inside a double-quoted string.
pub const KIND_PHYSICAL_NEWLINE: &str = "physical_newline_in_quoted_string";
/// `kind` of the issue for a string that never closes.
pub const KIND_UNTERMINATED: &str = "unterminated_string_literal";

const SPLIT_STRING_MESSAGE: &str =
    "string literal split across lines (classic Python triple-quote \\\"\\\\n\\\" corruption)";
const PHYSICAL_NEWLINE_MESSAGE: &str =
    "physical newline inside double-quoted string literal; expected escaped \\\\n sequence";
const UNTERMINATED_MESSAGE: &str = "unterminated double-quoted string literal";

/// One corruption finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// 1-based line number.
    pub line: usize,
    /// Stable kind name.
    pub kind: &'static str,
    /// Exact message text.
    pub message: &'static str,
}

impl Issue {
    fn to_json(&self) -> Value {
        json!({"line": self.line, "kind": self.kind, "message": self.message})
    }
}

/// Python `str.isspace()`, which is also what `\s` matches in a `str` pattern.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python `\w` for a `str` pattern.
fn py_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One character against a lowercase ASCII letter, as `re.IGNORECASE` does
/// (including the Unicode equivalents of `i` and `s`).
fn ci_eq(c: char, lower: char) -> bool {
    if c.to_ascii_lowercase() == lower {
        return true;
    }
    match lower {
        'i' => c == '\u{131}' || c == '\u{130}',
        's' => c == '\u{17f}',
        'k' => c == '\u{212a}',
        _ => false,
    }
}

fn literal_at(chars: &[char], pos: usize, literal: &str) -> Option<usize> {
    let mut at = pos;
    for expected in literal.chars() {
        if at >= chars.len() || !ci_eq(chars[at], expected) {
            return None;
        }
        at += 1;
    }
    Some(at)
}

/// `\s+` at `pos`: the end of the whitespace run, or `None` when empty.
fn spaces_at(chars: &[char], pos: usize) -> Option<usize> {
    let mut at = pos;
    while at < chars.len() && py_space(chars[at]) {
        at += 1;
    }
    (at > pos).then_some(at)
}

/// `\b` after a word character at `pos - 1`.
fn word_end_at(chars: &[char], pos: usize) -> bool {
    pos >= chars.len() || !py_word(chars[pos])
}

/// `\b` before a word character at `pos`.
fn word_start_at(chars: &[char], pos: usize) -> bool {
    pos == 0 || !py_word(chars[pos - 1])
}

/// Python `Path(path).suffix.lower() in JS_TS_SUFFIXES`.
pub fn is_js_ts_path(path: &str) -> bool {
    let name = path
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty() && *part != ".")
        .unwrap_or("");
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    if dot == 0 || dot + 1 == name.len() {
        return false;
    }
    let suffix = name[dot..].to_lowercase();
    JS_TS_SUFFIXES.contains(&suffix.as_str())
}

/// Python `JS_TS_PATH_RE.search`: `\.(?:js|jsx|ts|tsx|mjs|cjs)(?:\b|["'])`.
pub fn command_targets_js_ts(command: &str) -> bool {
    const EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs"];
    let chars: Vec<char> = command.chars().collect();
    for start in 0..chars.len() {
        if chars[start] != '.' {
            continue;
        }
        for extension in EXTENSIONS {
            if let Some(end) = literal_at(&chars, start + 1, extension) {
                let quote = end < chars.len() && matches!(chars[end], '"' | '\'');
                if quote || word_end_at(&chars, end) {
                    return true;
                }
            }
        }
    }
    false
}

/// Python `ESCAPE_LITERAL_HINT_RE.search`. Every alternative contains one of
/// the backslash sequences `\n`, `\r` or `\t`.
pub fn command_mentions_escape_literal(command: &str) -> bool {
    ["\\n", "\\r", "\\t"]
        .iter()
        .any(|hint| command.contains(hint))
}

/// Does the pattern at `index` of [`UNSAFE_SHELL_EDIT_PATTERNS`] match anywhere?
fn unsafe_pattern_matches(index: usize, chars: &[char]) -> bool {
    (0..chars.len()).any(|start| match index {
        // python3?\s+-c\b
        0 => literal_at(chars, start, "python").is_some_and(|after| {
            let with_three = (after < chars.len() && chars[after] == '3').then_some(after + 1);
            [with_three, Some(after)]
                .into_iter()
                .flatten()
                .any(|at| flag_after_spaces(chars, at, "-c"))
        }),
        // node\s+-e\b
        1 => literal_at(chars, start, "node").is_some_and(|at| flag_after_spaces(chars, at, "-e")),
        // \bsed\s+-i\b
        2 => {
            word_start_at(chars, start)
                && literal_at(chars, start, "sed")
                    .is_some_and(|at| flag_after_spaces(chars, at, "-i"))
        }
        // \bperl\s+-pi\b
        3 => {
            word_start_at(chars, start)
                && literal_at(chars, start, "perl")
                    .is_some_and(|at| flag_after_spaces(chars, at, "-pi"))
        }
        // \bawk\s+-i\s+inplace\b
        4 => {
            word_start_at(chars, start)
                && literal_at(chars, start, "awk")
                    .and_then(|at| spaces_at(chars, at))
                    .and_then(|at| literal_at(chars, at, "-i"))
                    .and_then(|at| spaces_at(chars, at))
                    .and_then(|at| literal_at(chars, at, "inplace"))
                    .is_some_and(|at| word_end_at(chars, at))
        }
        // powershell\s+-c\b
        5 => literal_at(chars, start, "powershell")
            .is_some_and(|at| flag_after_spaces(chars, at, "-c")),
        // pwsh\s+-c\b
        6 => literal_at(chars, start, "pwsh").is_some_and(|at| flag_after_spaces(chars, at, "-c")),
        _ => false,
    })
}

/// `\s+<flag>\b` at `pos`.
fn flag_after_spaces(chars: &[char], pos: usize, flag: &str) -> bool {
    spaces_at(chars, pos)
        .and_then(|at| literal_at(chars, at, flag))
        .is_some_and(|at| word_end_at(chars, at))
}

/// Red-flag `(pattern, reason)` pairs for unsafe shell edits near
/// escape-sensitive literals.
pub fn flag_unsafe_js_ts_shell_edit(command: &str) -> Vec<(&'static str, &'static str)> {
    if !(command_targets_js_ts(command) || command_mentions_escape_literal(command)) {
        return Vec::new();
    }
    let chars: Vec<char> = command.chars().collect();
    UNSAFE_SHELL_EDIT_PATTERNS
        .iter()
        .enumerate()
        .filter(|(index, _)| unsafe_pattern_matches(*index, &chars))
        .map(|(_, pair)| *pair)
        .collect()
}

/// Python `_scan_physical_newline_in_strings`.
pub fn scan_physical_newline_in_strings(content: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut start_line = 1;
    let mut line = 1;

    for ch in content.chars() {
        if ch == '\n' {
            if in_string {
                issues.push(Issue {
                    line: start_line,
                    kind: KIND_PHYSICAL_NEWLINE,
                    message: PHYSICAL_NEWLINE_MESSAGE,
                });
                in_string = false;
                escaped = false;
            }
            line += 1;
            continue;
        }
        if !in_string {
            if ch == '"' {
                in_string = true;
                escaped = false;
                start_line = line;
            }
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = false;
            escaped = false;
        }
    }
    if in_string {
        issues.push(Issue {
            line: start_line,
            kind: KIND_UNTERMINATED,
            message: UNTERMINATED_MESSAGE,
        });
    }
    issues
}

/// End of one `=\s*"\s*\n\s*";` match starting at `start`, if any.
///
/// The whitespace runs cannot contain `"`, so the regex reduces to: `=`,
/// spaces, `"`, a maximal space run holding at least one newline, then `";`.
fn split_assignment_end(chars: &[char], start: usize) -> Option<usize> {
    if chars[start] != '=' {
        return None;
    }
    let mut at = start + 1;
    while at < chars.len() && py_space(chars[at]) {
        at += 1;
    }
    if chars.get(at) != Some(&'"') {
        return None;
    }
    at += 1;
    let run_start = at;
    while at < chars.len() && py_space(chars[at]) {
        at += 1;
    }
    if !chars[run_start..at].contains(&'\n') {
        return None;
    }
    (chars.get(at) == Some(&'"') && chars.get(at + 1) == Some(&';')).then_some(at + 2)
}

/// Python `scan_escape_corruption`. An empty `path` skips the suffix filter.
pub fn scan_escape_corruption(content: &str, path: &str) -> Vec<Issue> {
    if !path.is_empty() && !is_js_ts_path(path) {
        return Vec::new();
    }

    let chars: Vec<char> = content.chars().collect();
    let mut issues = Vec::new();
    let mut at = 0;
    let mut newlines_before = 0;
    let mut counted_to = 0;
    while at < chars.len() {
        if let Some(end) = split_assignment_end(&chars, at) {
            newlines_before += chars[counted_to..at].iter().filter(|c| **c == '\n').count();
            counted_to = at;
            issues.push(Issue {
                line: newlines_before + 1,
                kind: KIND_SPLIT_STRING_ASSIGNMENT,
                message: SPLIT_STRING_MESSAGE,
            });
            at = end;
        } else {
            at += 1;
        }
    }
    issues.extend(scan_physical_newline_in_strings(content));

    let mut unique: Vec<Issue> = Vec::new();
    for issue in issues {
        if !unique
            .iter()
            .any(|seen| seen.line == issue.line && seen.kind == issue.kind)
        {
            unique.push(issue);
        }
    }
    unique
}

/// Handle one request. Every request has an `op` string.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let request = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be an object"))?;
    let op = required_string(request, "op")?;
    match op {
        "is_js_ts_path" => Ok(json!(is_js_ts_path(required_string(request, "path")?))),
        "command_targets_js_ts" => Ok(json!(command_targets_js_ts(command(request)?))),
        "command_mentions_escape_literal" => {
            Ok(json!(command_mentions_escape_literal(command(request)?)))
        }
        "flag_unsafe_js_ts_shell_edit" => Ok(Value::Array(
            flag_unsafe_js_ts_shell_edit(command(request)?)
                .into_iter()
                .map(|(token, reason)| json!([token, reason]))
                .collect(),
        )),
        "scan_physical_newline_in_strings" => Ok(issues_json(&scan_physical_newline_in_strings(
            required_string(request, "content")?,
        ))),
        "scan_escape_corruption" => {
            let path = optional_string(request, "path")?.unwrap_or("");
            Ok(issues_json(&scan_escape_corruption(
                required_string(request, "content")?,
                path,
            )))
        }
        other => Err(OpsError::invalid_request(format!("unknown op: {other}"))),
    }
}

fn issues_json(issues: &[Issue]) -> Value {
    Value::Array(issues.iter().map(Issue::to_json).collect())
}

/// `command`, where null or absent is Python's `command or ""`.
fn command(request: &Map<String, Value>) -> Result<&str, OpsError> {
    Ok(optional_string(request, "command")?.unwrap_or(""))
}

fn required_string<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a str, OpsError> {
    request
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request(format!("{key} must be a string")))
}

fn optional_string<'a>(
    request: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, OpsError> {
    match request.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(OpsError::invalid_request(format!(
            "{key} must be a string or null"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_rules_match_pathlib() {
        assert!(is_js_ts_path("src/App.TSX"));
        assert!(is_js_ts_path("a\\b\\c.mjs"));
        assert!(!is_js_ts_path(".js"));
        assert!(!is_js_ts_path("file.js."));
        assert!(is_js_ts_path("dir.js/"));
        assert!(is_js_ts_path("dir/file.js/"));
        assert!(!is_js_ts_path(""));
    }

    #[test]
    fn unknown_op_is_invalid_request() {
        assert!(handle(json!({"op": "nope"})).is_err());
        assert!(handle(json!([])).is_err());
    }
}

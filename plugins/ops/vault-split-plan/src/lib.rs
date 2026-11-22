//! Pure path classification and commit planning from the vault split helper.
//!
//! Callers provide changed paths and optional AGENTS.md text; this crate does
//! not access Git, the filesystem, the network, or Multica.

use std::collections::{BTreeMap, BTreeSet};

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};

/// Prefixes excluded from commit planning.
pub const EXCLUDED_PREFIXES: &[&str] = &[".pi-subagents/", ".pi/tmp/"];
/// Exact paths excluded from commit planning.
pub const EXCLUDED_EXACT: &[&str] = &[
    "import_dry.json",
    ".multica_import_dry.json",
    "games_out.json",
    "progress.md",
    "running",
    "nul",
];
/// Filename patterns excluded from commit planning.
pub const EXCLUDED_GLOBS: &[&str] = &[
    ".scratch-*",
    ".env",
    ".env.*",
    "credentials.json",
    "credentials.*.json",
];
/// The canonical order for fixed commit buckets.
pub const BUCKET_ORDER: &[&str] = &[
    "multica-spine",
    "pi-retention",
    "vault-root",
    "docs-root",
    "multica-strategy-design",
    "multica-strategy-feedback",
    "multica-strategy-calendar",
    "multica-strategy-progress",
    "multica-strategy-other",
    "structure",
    "fleeting",
    "personal-positioning",
    "x-creator",
];

/// A changed path and its original `git status --porcelain` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// The path used for classification and exclusion checks.
    pub path: String,
    /// The original porcelain line, retained for callers that need it.
    pub raw: String,
}

/// One planned commit bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketPlan {
    /// Stable bucket identifier.
    pub bucket_id: String,
    /// Sorted unique paths assigned to the bucket.
    pub files: Vec<String>,
    /// Commit message for this bucket.
    pub message: String,
}

/// Normalize Windows separators and remove one leading `./`, like Python.
pub fn normalize_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .strip_prefix("./")
        .unwrap_or(&normalized)
        .to_owned()
}

/// Whether the contents contain both Multica runtime markers.
pub fn agents_md_contains_multica_runtime(content: &str) -> bool {
    content.contains("<!-- BEGIN MULTICA-RUNTIME")
        && content.contains("<!-- END MULTICA-RUNTIME -->")
}

/// Remove the first well-formed managed Multica runtime block, if present.
pub fn strip_multica_runtime_block(content: &str) -> (String, bool) {
    const BEGIN: &str = "<!-- BEGIN MULTICA-RUNTIME";
    const END: &str = "<!-- END MULTICA-RUNTIME -->";

    if !agents_md_contains_multica_runtime(content) {
        return (content.to_owned(), false);
    }

    for (begin_at, _) in content.match_indices(BEGIN) {
        let after_begin = begin_at + BEGIN.len();
        let next = content[after_begin..].chars().next();
        // Python's `\b` after the final `E` requires the following character
        // to be absent or non-word (a Unicode letter/digit or underscore is a word).
        if next.is_some_and(|character| character.is_alphanumeric() || character == '_') {
            continue;
        }
        let Some(end_offset) = content[after_begin..].find(END) else {
            continue;
        };
        let end_at = after_begin + end_offset + END.len();

        let start_at = content[..begin_at]
            .char_indices()
            .rev()
            .take_while(|(_, character)| character.is_whitespace())
            .last()
            .map_or(begin_at, |(index, _)| index);
        let after_block = content[end_at..]
            .char_indices()
            .find(|(_, character)| !character.is_whitespace())
            .map_or(content.len(), |(index, _)| end_at + index);
        let remainder = format!("{}{}", &content[..start_at], &content[after_block..]);
        return (format!("{}\n", remainder.trim_end()), true);
    }

    (content.to_owned(), false)
}

/// Parse one porcelain line, choosing the destination path for renames.
pub fn parse_porcelain_line(line: &str) -> Option<StatusEntry> {
    let text = line.trim_end_matches('\n');
    if text.is_empty() {
        return None;
    }
    let raw = text.to_owned();
    let body = if text.chars().count() >= 4 {
        let byte_index = text
            .char_indices()
            .nth(3)
            .map_or(text.len(), |(index, _)| index);
        &text[byte_index..]
    } else {
        text
    };
    let body = body
        .split_once(" -> ")
        .map_or(body, |(_, new_path)| new_path);
    let path = normalize_path(body.trim_matches('"'));
    if path.is_empty() {
        return None;
    }
    Some(StatusEntry { path, raw })
}

/// Whether a normalized path is excluded from commit planning.
pub fn is_excluded_path(path: &str) -> bool {
    let normalized = normalize_path(path);
    if EXCLUDED_EXACT.contains(&normalized.as_str()) {
        return true;
    }
    if EXCLUDED_PREFIXES.iter().any(|prefix| {
        let directory = prefix.trim_end_matches('/');
        normalized == directory || normalized.starts_with(prefix)
    }) {
        return true;
    }
    let basename = normalized.rsplit('/').next().unwrap_or(&normalized);
    EXCLUDED_GLOBS.iter().any(|pattern| {
        // The source runs on Windows, where fnmatch.fnmatch applies normcase
        // and therefore matches these ASCII patterns without case sensitivity.
        glob_matches(&normalized, pattern) || glob_matches(basename, pattern)
    })
}

/// Classify a path into the same bucket as the vault helper.
pub fn classify_path(path: &str) -> String {
    let normalized = normalize_path(path);

    if normalized.starts_with(".multica-spine/") || normalized.starts_with(".multica/") {
        return "multica-spine".to_owned();
    }
    if matches!(
        normalized.as_str(),
        ".pi/.pi-retention-project.yaml" | ".pi-retention-project.yaml"
    ) {
        return "pi-retention".to_owned();
    }
    if matches!(
        normalized.as_str(),
        "AGENTS.md" | "CONTEXT-MAP.md" | "VENT.md"
    ) {
        return "vault-root".to_owned();
    }
    if normalized.starts_with(".pi/skills/") {
        let parts: Vec<_> = normalized.split('/').collect();
        return if parts.len() >= 3 && !parts[2].is_empty() {
            format!("skill-{}", parts[2])
        } else {
            "skills".to_owned()
        };
    }
    if normalized.starts_with("4_Project/OSS/") && normalized.contains("/Issues/") {
        let parts: Vec<_> = normalized.split('/').collect();
        return if parts.len() >= 4 && !parts[2].is_empty() {
            format!("oss-issues-{}", parts[2])
        } else {
            "oss-issues".to_owned()
        };
    }
    if normalized.starts_with("4_Project/Multica-Agent-Strategy/Data/feedback-events/") {
        return "multica-strategy-feedback".to_owned();
    }
    if normalized.starts_with("4_Project/Multica-Agent-Strategy/Data/AI-Execution-Calendar/") {
        return "multica-strategy-calendar".to_owned();
    }
    if normalized.starts_with("4_Project/Multica-Agent-Strategy/Data/morning-routine-failures/")
        || normalized.starts_with("4_Project/Multica-Agent-Strategy/Progress/")
        || normalized.starts_with("4_Project/Multica-Agent-Strategy/Incubations/")
    {
        return "multica-strategy-progress".to_owned();
    }
    if normalized.starts_with("4_Project/Multica-Agent-Strategy/Design/")
        || normalized.starts_with("4_Project/Multica-Agent-Strategy/docs/adr/")
    {
        return "multica-strategy-design".to_owned();
    }
    if normalized.starts_with("4_Project/Multica-Agent-Strategy/") {
        return "multica-strategy-other".to_owned();
    }
    if normalized.starts_with("4_Project/X-Creator-Positioning/") {
        return "x-creator".to_owned();
    }
    if normalized.starts_with("1_Fleeting/") {
        return "fleeting".to_owned();
    }
    if normalized.starts_with("4_Project/Personal-Positioning/") {
        return "personal-positioning".to_owned();
    }
    if normalized.starts_with("5_Structure/") {
        return "structure".to_owned();
    }
    if normalized.starts_with("docs/") {
        return "docs-root".to_owned();
    }
    if normalized.starts_with("4_Project/") {
        let parts: Vec<_> = normalized.split('/').collect();
        if parts.len() >= 2 && !parts[1].is_empty() {
            let key = parts[1]
                .split(|character: char| {
                    !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-')
                })
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("-")
                .to_lowercase();
            return format!("project-{}", if key.is_empty() { "unknown" } else { &key });
        }
        return "project-unknown".to_owned();
    }
    "other".to_owned()
}

/// Return the ordering key used for fixed and dynamic buckets.
pub fn bucket_sort_key(bucket_id: &str) -> (usize, String) {
    if let Some(index) = BUCKET_ORDER.iter().position(|bucket| *bucket == bucket_id) {
        return (index, bucket_id.to_owned());
    }
    let index = if bucket_id.starts_with("skill-") {
        20
    } else if bucket_id.starts_with("oss-issues-") {
        30
    } else if bucket_id.starts_with("project-") {
        40
    } else if bucket_id == "other" {
        100
    } else {
        50
    };
    (index, bucket_id.to_owned())
}

/// Produce the commit subject for one bucket.
pub fn summarize_bucket(bucket_id: &str, files: &[String], today: Option<&str>) -> String {
    let current_day;
    let day = if let Some(today) = today {
        today
    } else {
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        current_day = format!(
            "{:04}-{:02}-{:02}",
            now.year(),
            u8::from(now.month()),
            now.day()
        );
        &current_day
    };
    match bucket_id {
        "multica-spine" => format!("chore(multica-spine): refresh spine state ({day})"),
        "pi-retention" => "chore: refresh pi retention timestamp".to_owned(),
        "vault-root" => format!("docs: update vault root pointers ({day})"),
        "docs-root" => format!("docs: update shared docs ({day})"),
        "multica-strategy-feedback" => {
            format!("chore(multica-agent-strategy): record feedback events ({day})")
        }
        "multica-strategy-calendar" => {
            format!("chore(multica-agent-strategy): update AI execution calendar ({day})")
        }
        "multica-strategy-progress" => {
            format!("chore(multica-agent-strategy): update progress notes ({day})")
        }
        "multica-strategy-design" => {
            format!("docs(multica-agent-strategy): update design docs ({day})")
        }
        "multica-strategy-other" => {
            format!("chore(multica-agent-strategy): update strategy artifacts ({day})")
        }
        "structure" => format!("docs(structure): refresh structure notes ({day})"),
        "fleeting" => format!("fleeting: capture vault intake ({day})"),
        "personal-positioning" => {
            format!("docs(personal-positioning): update positioning docs ({day})")
        }
        "x-creator" => format!("chore(x-creator): update X creator artifacts ({day})"),
        _ if bucket_id.starts_with("skill-") => {
            let prefix = if files.iter().any(|path| path.contains("/scripts/")) {
                "feat"
            } else {
                "docs"
            };
            format!("{prefix}(skills): update {}", &bucket_id["skill-".len()..])
        }
        _ if bucket_id.starts_with("oss-issues-") => {
            format!(
                "docs(oss): sync {} issue metadata",
                &bucket_id["oss-issues-".len()..]
            )
        }
        _ if bucket_id.starts_with("project-") => format!(
            "docs({}): update project notes ({day})",
            &bucket_id["project-".len()..]
        ),
        "other" => format!("chore: update vault files ({day})"),
        _ => format!("chore: update {bucket_id} ({day})"),
    }
}

/// Group eligible entries by bucket and return excluded paths separately.
pub fn group_status_entries(
    entries: &[StatusEntry],
) -> (BTreeMap<String, Vec<String>>, Vec<String>) {
    let mut buckets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut excluded = BTreeSet::new();

    for entry in entries {
        if is_excluded_path(&entry.path) {
            excluded.insert(entry.path.clone());
            continue;
        }
        buckets
            .entry(classify_path(&entry.path))
            .or_default()
            .insert(entry.path.clone());
    }

    (
        buckets
            .into_iter()
            .map(|(bucket, paths)| (bucket, paths.into_iter().collect()))
            .collect(),
        excluded.into_iter().collect(),
    )
}

/// Build deterministic, ordered commit plans from changed paths.
pub fn build_bucket_plans(entries: &[StatusEntry], today: Option<&str>) -> Vec<BucketPlan> {
    let (grouped, _) = group_status_entries(entries);
    let mut buckets: Vec<_> = grouped.into_iter().collect();
    buckets.sort_by_key(|(bucket, _)| bucket_sort_key(bucket));
    buckets
        .into_iter()
        .map(|(bucket_id, files)| BucketPlan {
            message: summarize_bucket(&bucket_id, &files, today),
            bucket_id,
            files,
        })
        .collect()
}

/// Handle the crate's stdio JSON operation request.
///
/// Each request has an `op` field: `normalize_path`, `is_excluded_path`,
/// `exclude_paths`, `classify_path`, `parse_porcelain_line`,
/// `parse_porcelain_lines`, `agents_md_contains_multica_runtime`,
/// `strip_multica_runtime_block`, `group_status_entries`, or
/// `build_bucket_plans`.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let request = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be an object"))?;
    let op = required_string(request, "op")?;
    match op {
        "normalize_path" => Ok(json!(normalize_path(required_string(request, "path")?))),
        "is_excluded_path" => Ok(json!(is_excluded_path(required_string(request, "path")?))),
        "exclude_paths" => {
            let paths = string_array(request, "paths")?;
            Ok(json!(paths
                .into_iter()
                .map(is_excluded_path)
                .collect::<Vec<_>>()))
        }
        "classify_path" => Ok(json!(classify_path(required_string(request, "path")?))),
        "parse_porcelain_line" => Ok(entry_json(parse_porcelain_line(required_string(
            request, "line",
        )?))),
        "parse_porcelain_lines" => {
            let lines = string_array(request, "lines")?;
            Ok(json!(lines
                .into_iter()
                .map(parse_porcelain_line)
                .map(entry_json)
                .collect::<Vec<_>>()))
        }
        "agents_md_contains_multica_runtime" => Ok(json!(agents_md_contains_multica_runtime(
            required_string(request, "content")?
        ))),
        "strip_multica_runtime_block" => {
            let (content, stripped) =
                strip_multica_runtime_block(required_string(request, "content")?);
            Ok(json!({ "content": content, "stripped": stripped }))
        }
        "group_status_entries" => {
            let entries = status_entries(request)?;
            let (groups, excluded) = group_status_entries(&entries);
            let groups: Map<String, Value> = groups
                .into_iter()
                .map(|(bucket, files)| (bucket, json!(files)))
                .collect();
            Ok(json!({ "groups": groups, "excluded": excluded }))
        }
        "build_bucket_plans" => {
            let entries = status_entries(request)?;
            let today = optional_string(request, "today")?.filter(|value| !value.is_empty());
            let plans = build_bucket_plans(&entries, today);
            Ok(json!({
                "plans": plans.into_iter().map(|plan| json!({
                    "bucket_id": plan.bucket_id,
                    "files": plan.files,
                    "message": plan.message,
                })).collect::<Vec<_>>(),
                "excluded": group_status_entries(&entries).1,
            }))
        }
        _ => Err(OpsError::invalid_request(format!(
            "unknown operation: {op}"
        ))),
    }
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

fn string_array<'a>(request: &'a Map<String, Value>, key: &str) -> Result<Vec<&'a str>, OpsError> {
    let values = request
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| OpsError::invalid_request(format!("{key} must be an array")))?;
    values
        .iter()
        .map(|value| {
            value.as_str().ok_or_else(|| {
                OpsError::invalid_request(format!("{key} must contain only strings"))
            })
        })
        .collect()
}

fn status_entries(request: &Map<String, Value>) -> Result<Vec<StatusEntry>, OpsError> {
    let entries = request
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| OpsError::invalid_request("entries must be an array"))?;
    entries
        .iter()
        .map(|entry| {
            let entry = entry
                .as_object()
                .ok_or_else(|| OpsError::invalid_request("each entry must be an object"))?;
            Ok(StatusEntry {
                path: required_string(entry, "path")?.to_owned(),
                raw: required_string(entry, "raw")?.to_owned(),
            })
        })
        .collect()
}

fn entry_json(entry: Option<StatusEntry>) -> Value {
    entry.map_or(
        Value::Null,
        |entry| json!({ "path": entry.path, "raw": entry.raw }),
    )
}

fn glob_matches(value: &str, pattern: &str) -> bool {
    // Python fnmatch.fnmatch on the Windows vault is case-insensitive.
    let value: Vec<_> = value.to_lowercase().chars().collect();
    let pattern: Vec<_> = pattern.to_lowercase().chars().collect();
    let (mut value_index, mut pattern_index) = (0, 0);
    let (mut star_pattern, mut star_value) = (None, 0);

    while value_index < value.len() {
        if pattern.get(pattern_index) == Some(&value[value_index])
            || pattern.get(pattern_index) == Some(&'?')
        {
            value_index += 1;
            pattern_index += 1;
        } else if pattern.get(pattern_index) == Some(&'*') {
            star_pattern = Some(pattern_index);
            pattern_index += 1;
            star_value = value_index;
        } else if let Some(star) = star_pattern {
            pattern_index = star + 1;
            star_value += 1;
            value_index = star_value;
        } else {
            return false;
        }
    }
    pattern[pattern_index..]
        .iter()
        .all(|character| *character == '*')
}

//! Deterministic Markdown parsing and validation for the managed-project registry.
//!
//! This mirrors the corresponding pure parsing and joining behavior in
//! `managed_projects_registry.py`. The caller supplies the already-loaded
//! Markdown so this crate has no filesystem or network access.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use serde_json::{json, Value};

use agentmesh_ops_kit::OpsError;

const PROJECTS_HEADING: &str = "## Projects";
const PRODUCT_BUILD_HEADING: &str = "## Product build projects";
const PORTFOLIO_HEADING: &str = "## Portfolio / meta project";
const IMPORT_HEADING: &str = "## Local Issue Import mapping";

const PROJECTS_HEADERS: &[&str] = &[
    "project_key",
    "multica_project_id",
    "project_name",
    "repo/workspace",
    "status",
    "priority",
    "lead",
    "notes",
];

const IMPORT_HEADERS: &[&str] = &[
    "project_key",
    "obsidian_project_path",
    "local_issue_dir",
    "local_issue_glob",
    "local_issue_import_enabled",
];

const PORTFOLIO_HEADERS: &[&str] = &[
    "project_key",
    "multica_project_id",
    "project_name",
    "status",
    "priority",
    "lead",
    "role",
];

/// Parsed Markdown table row keyed by its header names.
pub type ParsedRow = BTreeMap<String, String>;

/// A registry validation or table-parsing failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryValidationError(String);

impl RegistryValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for RegistryValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RegistryValidationError {}

/// A normalized managed project joined from the registry's project and import tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManagedProject {
    /// Project's stable registry key.
    pub key: String,
    /// Multica project UUID, or an empty string for a missing ID.
    pub multica_project_id: String,
    /// Human-readable project name.
    pub project_name: String,
    /// Repository or workspace label.
    pub workspace_path: String,
    /// Project status from its source table.
    pub status: String,
    /// Project priority from its source table.
    pub priority: String,
    /// Project lead from its source table.
    pub lead: String,
    /// Notes or the portfolio role, depending on the source table.
    pub notes: String,
    /// Obsidian project path from the import-mapping table.
    pub obsidian_project_path: String,
    /// Local issue directory from the import-mapping table.
    pub local_issue_dir: String,
    /// Local issue filename glob from the import-mapping table.
    pub local_issue_glob: String,
    /// Whether local issue import is enabled.
    pub import_enabled: bool,
}

/// Trim a table cell and remove one pair of surrounding Markdown backticks.
pub fn clean_cell(cell: &str) -> String {
    let mut text = cell.trim();
    if text.starts_with('`') && text.ends_with('`') {
        text = &text[1..text.len() - 1];
    }
    text.trim().to_owned()
}

fn collect_table_lines(text: &str, heading: &str) -> Result<Vec<String>, RegistryValidationError> {
    let Some(index) = text.find(heading) else {
        return Err(RegistryValidationError::new(format!(
            "heading missing: {heading}"
        )));
    };
    let mut lines = text[index..].lines();
    let _heading_line = lines.next();
    let mut table_lines = Vec::new();
    let mut started = false;

    for line in lines {
        if line.starts_with("## ") && started {
            break;
        }
        if line.trim().starts_with('|') {
            started = true;
            table_lines.push(line.trim_end().to_owned());
        } else if started && line.trim().is_empty() {
            break;
        }
    }

    if table_lines.len() < 2 {
        return Err(RegistryValidationError::new(format!(
            "table missing or too short after heading: {heading}"
        )));
    }
    Ok(table_lines)
}

/// Parse a Markdown table immediately following `heading`.
///
/// As in the Python implementation, table splitting is a literal `|` split;
/// escaped pipes are not treated specially.
pub fn parse_table_after_heading(
    text: &str,
    heading: &str,
) -> Result<(Vec<String>, Vec<ParsedRow>), RegistryValidationError> {
    let lines = collect_table_lines(text, heading)?;
    let headers = split_table_line(&lines[0])
        .into_iter()
        .map(|cell| clean_cell(&cell))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();

    for line in lines.iter().skip(2) {
        let cells = split_table_line(line)
            .into_iter()
            .map(|cell| clean_cell(&cell))
            .collect::<Vec<_>>();
        if cells.len() != headers.len() {
            return Err(RegistryValidationError::new(format!(
                "row column mismatch in {heading}: expected {} cells, got {} :: {}",
                headers.len(),
                cells.len(),
                line.trim()
            )));
        }
        let row = headers.iter().cloned().zip(cells).collect();
        rows.push(row);
    }
    Ok((headers, rows))
}

fn split_table_line(line: &str) -> Vec<String> {
    line.trim_matches('|')
        .split('|')
        .map(str::to_owned)
        .collect()
}

/// Validate a table's headers against the exact expected sequence.
pub fn validate_headers(
    actual: &[String],
    expected: &[&str],
    table_name: &str,
) -> Result<(), RegistryValidationError> {
    if actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        return Ok(());
    }
    Err(RegistryValidationError::new(format!(
        "unexpected headers in {table_name}: expected {}, got {}",
        python_list_repr(expected.iter().map(|header| (*header).to_owned())),
        python_list_repr(actual.iter().cloned())
    )))
}

fn python_list_repr<'a>(values: impl Iterator<Item = String> + 'a) -> String {
    let contents = values
        .map(python_string_repr)
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{contents}]")
}

fn python_string_repr(value: String) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut result = String::with_capacity(value.len() + 2);
    result.push(quote);
    for character in value.chars() {
        match character {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            character if character == quote => {
                result.push('\\');
                result.push(character);
            }
            character => result.push(character),
        }
    }
    result.push(quote);
    result
}

/// Parse a Python-compatible boolean value (`true` or `false`, case-insensitive).
pub fn parse_bool(value: &str) -> Result<bool, RegistryValidationError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(RegistryValidationError::new(format!(
            "invalid boolean value: {value}"
        ))),
    }
}

/// Normalize a missing Multica project ID to the empty string; otherwise trim it.
pub fn normalize_multica_project_id(project_id: &str) -> String {
    if ["", "n/a", "na", "none", "null"].contains(&project_id.trim().to_ascii_lowercase().as_str())
    {
        String::new()
    } else {
        project_id.trim().to_owned()
    }
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}

fn validate_projects_rows(rows: &[ParsedRow]) -> Result<(), RegistryValidationError> {
    let mut seen = Vec::<&str>::new();
    for row in rows {
        let key = row["project_key"].as_str();
        if key.is_empty() {
            return Err(RegistryValidationError::new(
                "blank project_key in Projects table",
            ));
        }
        if seen.contains(&key) {
            return Err(RegistryValidationError::new(format!(
                "duplicate project_key in Projects table: {key}"
            )));
        }
        seen.push(key);
        let project_id = &row["multica_project_id"];
        if !["", "n/a", "na", "none", "null"]
            .contains(&project_id.trim().to_ascii_lowercase().as_str())
            && !is_uuid(project_id)
        {
            return Err(RegistryValidationError::new(format!(
                "invalid multica_project_id for {key}: {project_id}"
            )));
        }
        for field in [
            "project_name",
            "repo/workspace",
            "status",
            "priority",
            "lead",
        ] {
            if row[field].is_empty() {
                return Err(RegistryValidationError::new(format!(
                    "blank {field} for {key} in Projects table"
                )));
            }
        }
    }
    Ok(())
}

fn validate_import_rows(rows: &[ParsedRow]) -> Result<(), RegistryValidationError> {
    let mut seen = Vec::<&str>::new();
    for row in rows {
        let key = row["project_key"].as_str();
        if key.is_empty() {
            return Err(RegistryValidationError::new(
                "blank project_key in Local Issue Import mapping",
            ));
        }
        if seen.contains(&key) {
            return Err(RegistryValidationError::new(format!(
                "duplicate project_key in Local Issue Import mapping: {key}"
            )));
        }
        seen.push(key);
        for field in [
            "obsidian_project_path",
            "local_issue_dir",
            "local_issue_glob",
            "local_issue_import_enabled",
        ] {
            if row[field].is_empty() {
                return Err(RegistryValidationError::new(format!(
                    "blank {field} for {key} in Local Issue Import mapping"
                )));
            }
        }
        parse_bool(&row["local_issue_import_enabled"])?;
    }
    Ok(())
}

/// Validate required tables and optional build/portfolio tables.
pub fn validate_registry(text: &str) -> Result<Value, RegistryValidationError> {
    let (project_headers, project_rows) = parse_table_after_heading(text, PROJECTS_HEADING)?;
    let (import_headers, import_rows) = parse_table_after_heading(text, IMPORT_HEADING)?;
    validate_headers(&project_headers, PROJECTS_HEADERS, PROJECTS_HEADING)?;
    validate_headers(&import_headers, IMPORT_HEADERS, IMPORT_HEADING)?;
    validate_projects_rows(&project_rows)?;
    validate_import_rows(&import_rows)?;

    let mut extra_project_keys = Vec::<String>::new();
    let product_build_rows = optional_project_table(text, PRODUCT_BUILD_HEADING, PROJECTS_HEADERS);
    extra_project_keys.extend(
        product_build_rows
            .iter()
            .map(|row| row["project_key"].clone()),
    );

    let portfolio_rows = optional_portfolio_table(text);
    for row in &portfolio_rows {
        if !row["project_key"].is_empty() && is_uuid(&row["multica_project_id"]) {
            extra_project_keys.push(row["project_key"].clone());
        }
    }

    let mut project_keys = project_rows
        .iter()
        .map(|row| row["project_key"].clone())
        .collect::<Vec<_>>();
    project_keys.extend(extra_project_keys);
    let mut import_keys = import_rows
        .iter()
        .map(|row| row["project_key"].clone())
        .collect::<Vec<_>>();
    project_keys.sort();
    project_keys.dedup();
    import_keys.sort();
    import_keys.dedup();
    let missing = import_keys
        .into_iter()
        .filter(|key| !project_keys.contains(key))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(RegistryValidationError::new(format!(
            "import mapping has project_key values missing from Projects table: {}",
            missing.join(", ")
        )));
    }

    Ok(json!({
        "projects_count": project_rows.len(),
        "product_build_count": product_build_rows.len(),
        "import_mapping_count": import_rows.len(),
    }))
}

fn optional_project_table(text: &str, heading: &str, expected: &[&str]) -> Vec<ParsedRow> {
    let result = (|| {
        let (headers, rows) = parse_table_after_heading(text, heading)?;
        validate_headers(&headers, expected, heading)?;
        validate_projects_rows(&rows)?;
        Ok::<_, RegistryValidationError>(rows)
    })();
    result.unwrap_or_default()
}

fn optional_portfolio_table(text: &str) -> Vec<ParsedRow> {
    let result = (|| {
        let (headers, rows) = parse_table_after_heading(text, PORTFOLIO_HEADING)?;
        validate_headers(&headers, PORTFOLIO_HEADERS, PORTFOLIO_HEADING)?;
        Ok::<_, RegistryValidationError>(rows)
    })();
    result.unwrap_or_default()
}

/// Join the registry's four tables into Python-compatible managed-project rows.
pub fn load_managed_projects(
    text: &str,
) -> Result<BTreeMap<String, ManagedProject>, RegistryValidationError> {
    validate_registry(text)?;
    let (_, project_rows) = parse_table_after_heading(text, PROJECTS_HEADING)?;
    let (_, import_rows) = parse_table_after_heading(text, IMPORT_HEADING)?;

    let mut projects_by_key = project_rows
        .into_iter()
        .map(|row| (row["project_key"].clone(), row))
        .collect::<BTreeMap<_, _>>();
    if let Ok((_, product_rows)) = parse_table_after_heading(text, PRODUCT_BUILD_HEADING) {
        for row in product_rows {
            projects_by_key.insert(row["project_key"].clone(), row);
        }
    }
    if let Ok((_, portfolio_rows)) = parse_table_after_heading(text, PORTFOLIO_HEADING) {
        for row in portfolio_rows {
            let mut project = ParsedRow::new();
            project.insert("project_key".to_owned(), row["project_key"].clone());
            project.insert(
                "multica_project_id".to_owned(),
                row["multica_project_id"].clone(),
            );
            project.insert("project_name".to_owned(), row["project_name"].clone());
            project.insert("repo/workspace".to_owned(), String::new());
            project.insert("status".to_owned(), row["status"].clone());
            project.insert("priority".to_owned(), row["priority"].clone());
            project.insert("lead".to_owned(), row["lead"].clone());
            project.insert("notes".to_owned(), row["role"].clone());
            projects_by_key.insert(row["project_key"].clone(), project);
        }
    }

    let mut managed = BTreeMap::new();
    for import in import_rows {
        let key = &import["project_key"];
        let project = &projects_by_key[key];
        managed.insert(
            key.clone(),
            ManagedProject {
                key: key.clone(),
                multica_project_id: normalize_multica_project_id(&project["multica_project_id"]),
                project_name: project["project_name"].clone(),
                workspace_path: project["repo/workspace"].clone(),
                status: project["status"].clone(),
                priority: project["priority"].clone(),
                lead: project["lead"].clone(),
                notes: project["notes"].clone(),
                obsidian_project_path: import["obsidian_project_path"].clone(),
                local_issue_dir: import["local_issue_dir"].clone(),
                local_issue_glob: if import["local_issue_glob"].is_empty() {
                    "*.md".to_owned()
                } else {
                    import["local_issue_glob"].clone()
                },
                import_enabled: parse_bool(&import["local_issue_import_enabled"])?,
            },
        );
    }
    Ok(managed)
}

/// Handle the stdio contract: `{"registry_markdown":"..."}` to a project map.
pub fn handle_request(request: Value) -> Result<Value, OpsError> {
    let Some(object) = request.as_object() else {
        return Err(OpsError::invalid_request("request must be a JSON object"));
    };
    let Some(markdown) = object.get("registry_markdown").and_then(Value::as_str) else {
        return Err(OpsError::invalid_request(
            "registry_markdown must be a string",
        ));
    };
    let projects =
        load_managed_projects(markdown).map_err(|error| OpsError::failed(error.to_string()))?;
    serde_json::to_value(projects)
        .map_err(|error| OpsError::failed(format!("cannot serialize managed projects: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_wrapped_cells_and_booleans() {
        assert_eq!(clean_cell("  ` value `  "), "value");
        assert!(parse_bool(" TRUE ").unwrap());
        assert!(!parse_bool("False").unwrap());
        assert_eq!(normalize_multica_project_id(" N/A "), "");
        assert_eq!(
            normalize_multica_project_id(" 11111111-2222-3333-4444-555555555555 "),
            "11111111-2222-3333-4444-555555555555"
        );
    }

    #[test]
    fn preserves_the_python_parser_error_for_escaped_pipes() {
        let text = "## Projects\n| a | b |\n| - | - |\n| left \\| right | value |\n";
        let error = parse_table_after_heading(text, PROJECTS_HEADING).unwrap_err();
        assert!(error.to_string().contains("expected 2 cells, got 3"));
    }
}

//! Python-compatible selection and hashing of task-specification Markdown.

use sha2::{Digest, Sha256};
use std::mem;

use serde_json::Value;
use unicode_casefold::UnicodeCaseFold;

use agentmesh_ops_kit::OpsError;

const SPEC_HEADINGS: &[&str] = &[
    "what to build",
    "goal",
    "scope",
    "task",
    "what to do",
    "deliverables",
    "constraints",
    "non-goals",
    "out of scope",
    "acceptance criteria",
    "success criteria",
    "definition of done",
    "目的",
    "実装範囲",
    "完了条件",
    "明示的に対象外",
];

const OPERATIONAL_HEADINGS: &[&str] = &[
    "notes",
    "evidence",
    "verification report",
    "multica",
    "scope shipped",
    "検証済み",
    "結果",
    "検証",
    "検証結果",
    "調査結果",
    "供給結果",
];

/// Return the normalized heading vocabulary name, removing a qualifier and
/// optional trailing ATX closing hashes.
#[must_use]
pub fn heading_name(title: &str) -> String {
    let title = case_fold(strip_atx_closing_hashes(title).trim());
    let end = title
        .char_indices()
        .find_map(|(index, character)| matches!(character, '(' | '（').then_some(index))
        .unwrap_or(title.len());
    title[..end].trim().to_owned()
}

/// Return the normalized specification snapshot, or `None` if no recognized
/// specification heading exists.
#[must_use]
pub fn normalized_task_spec(text: &str) -> Option<String> {
    let normalized_newlines = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut text = strip_frontmatter(&normalized_newlines)?;
    text = remove_review_lessons(&text);

    let mut sections = Vec::new();
    let mut current: Option<Vec<String>> = None;
    let mut selected_level = 0;
    let mut excluded_level = None;
    let mut fence: Option<(char, usize)> = None;

    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches(char::is_whitespace);
        let fence_match = parse_fence(line);

        if let Some((fence_character, fence_length)) = fence {
            if excluded_level.is_none() {
                append_line(&mut current, line);
            }
            if let Some((closing_character, closing_length, closing_rest)) = fence_match {
                if closing_character == fence_character
                    && closing_length >= fence_length
                    && closing_rest.trim().is_empty()
                {
                    fence = None;
                }
            }
            continue;
        }

        if let Some((fence_character, fence_length, _)) = fence_match {
            fence = Some((fence_character, fence_length));
            append_line(&mut current, line);
            continue;
        }

        if let Some((level, title)) = parse_heading(line) {
            if current.is_some() && level <= selected_level {
                sections.push(take_section(&mut current));
            }
            if excluded_level.is_some_and(|excluded| level <= excluded) {
                excluded_level = None;
            }

            let name = heading_name(title);
            if OPERATIONAL_HEADINGS.contains(&name.as_str()) {
                excluded_level = Some(level);
            }
            if excluded_level.is_some() {
                continue;
            }

            if current.is_none() && SPEC_HEADINGS.contains(&name.as_str()) {
                selected_level = level;
                let section_title = case_fold(strip_atx_closing_hashes(title).trim());
                current = Some(vec![format!("## {section_title}")]);
                continue;
            }
        }

        if excluded_level.is_none() {
            append_line(&mut current, line);
        }
    }

    if current.is_some() {
        sections.push(take_section(&mut current));
    }
    if sections.is_empty() {
        return None;
    }

    sections.sort();
    Some(sections.join("\n\n") + "\n")
}

fn case_fold(text: &str) -> String {
    text.case_fold().collect()
}

/// Return the `sha256:` digest for a task specification, or `None` when it has
/// no recognized specification heading.
#[must_use]
pub fn task_spec_hash(text: &str) -> Option<String> {
    let snapshot = normalized_task_spec(text)?;
    let digest = Sha256::digest(snapshot.as_bytes());
    Some(format!("sha256:{}", hex::encode(digest)))
}

/// Handle a stdio request shaped as `{"text":"<Markdown>"}`.
pub fn handle_request(request: Value) -> Result<Value, OpsError> {
    let text = request
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("text must be a string"))?;
    Ok(task_spec_hash(text).map_or(Value::Null, Value::String))
}

fn strip_atx_closing_hashes(title: &str) -> &str {
    let without_trailing_horizontal_space = title.trim_end_matches([' ', '\t']);
    let hash_start = without_trailing_horizontal_space
        .char_indices()
        .rev()
        .find_map(|(index, character)| (character != '#').then_some(index + character.len_utf8()))
        .unwrap_or(0);
    let hashes = &without_trailing_horizontal_space[hash_start..];
    if hashes.is_empty() || !hashes.chars().all(|character| character == '#') {
        return title;
    }

    let prefix = &without_trailing_horizontal_space[..hash_start];
    if !prefix.ends_with([' ', '\t']) {
        return title;
    }
    prefix.trim_end_matches([' ', '\t'])
}

fn strip_frontmatter(text: &str) -> Option<String> {
    let Some(rest) = text.strip_prefix("---\n") else {
        return Some(text.to_owned());
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        if line_without_newline.trim_end_matches([' ', '\t']) == "---" {
            return Some(
                rest[offset + line.len()..]
                    .trim_start_matches('\n')
                    .to_owned(),
            );
        }
        offset += line.len();
    }
    None
}

fn remove_review_lessons(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some((start, marker_end)) = find_review_marker(text, "start", cursor) {
        result.push_str(&text[cursor..start]);
        if let Some((_, end)) = find_review_marker(text, "end", marker_end) {
            cursor = end;
        } else {
            cursor = text.len();
            break;
        }
    }
    result.push_str(&text[cursor..]);
    result
}

fn find_review_marker(text: &str, marker: &str, from: usize) -> Option<(usize, usize)> {
    let mut search_from = from;
    while let Some(relative_start) = text[search_from..].find("<!--") {
        let start = search_from + relative_start;
        let mut position = start + 4;
        position = skip_whitespace(text, position);
        let expected = format!("review-lessons:{marker}");
        if text[position..].starts_with(&expected) {
            position += expected.len();
            position = skip_whitespace(text, position);
            if text[position..].starts_with("-->") {
                return Some((start, position + 3));
            }
        }
        search_from = start + 4;
    }
    None
}

fn skip_whitespace(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find_map(|(offset, character)| (!character.is_whitespace()).then_some(start + offset))
        .unwrap_or(text.len())
}

fn parse_fence(line: &str) -> Option<(char, usize, &str)> {
    let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
    if indentation > 3 {
        return None;
    }
    let rest = &line[indentation..];
    let character = rest.chars().next()?;
    if !matches!(character, '`' | '~') {
        return None;
    }
    let length = rest
        .chars()
        .take_while(|candidate| *candidate == character)
        .count();
    (length >= 3).then_some((character, length, &rest[length..]))
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
    if indentation > 3 {
        return None;
    }
    let rest = &line[indentation..];
    let level = rest.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let separator = &rest[level..];
    if !separator.starts_with([' ', '\t']) {
        return None;
    }
    let title_start = separator
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    let title = &separator[title_start..];
    if title.is_empty() {
        return None;
    }
    Some((level, title.trim_end_matches(char::is_whitespace)))
}

fn append_line(current: &mut Option<Vec<String>>, line: &str) {
    if let Some(section) = current {
        section.push(line.to_owned());
    }
}

fn take_section(current: &mut Option<Vec<String>>) -> String {
    mem::take(current)
        .expect("a current section was checked")
        .join("\n")
        .trim_matches('\n')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn heading_name_strips_qualifiers_and_atx_closers() {
        assert_eq!(
            heading_name("  WHAT TO BUILD (small slice) ###  "),
            "what to build"
        );
        assert_eq!(heading_name("実装範囲（説明）"), "実装範囲");
        assert_eq!(heading_name("What to build###"), "what to build###");
    }

    #[test]
    fn heading_name_uses_full_unicode_case_folding() {
        assert_eq!(heading_name("ſcope"), "scope");
        assert_eq!(heading_name("Succeẞ criteria"), "success criteria");
    }

    #[test]
    fn normalizes_frontmatter_sections_and_operational_subtrees() {
        let text = "---\r\nstatus: todo\r\n---\r\n## Goal\r\n Build it.  \r\n### Details\r\nKeep this.\r\n### Notes\r\nIgnore this.\r\n";
        assert_eq!(
            normalized_task_spec(text).as_deref(),
            Some("## goal\n Build it.\n### Details\nKeep this.\n")
        );
    }

    #[test]
    fn operational_metadata_and_review_lessons_do_not_change_the_hash() {
        let original = "---\nstatus: ready\nowner: synthetic\n---\n## Scope\nKeep this.\n## Notes\nProgress.\n";
        let enriched = "---\nstatus: blocked\nowner: another\n---\n## Scope\nKeep this.\n## Notes\nChanged log.\n<!-- review-lessons:start -->\n## Acceptance criteria\nInjected advice.\n<!-- review-lessons:end -->\n";
        assert_eq!(task_spec_hash(original), task_spec_hash(enriched));
    }

    #[test]
    fn acceptance_changes_and_section_reordering_are_observable() {
        let original = "## Goal\nBuild it.\n\n## Acceptance criteria\nResult is correct.\n";
        let changed = original.replace("Result is correct", "Result is bounded");
        let reordered =
            "## Acceptance criteria  \r\nResult is correct. \t\r\n\r\n## Goal\r\nBuild it.\t\r\n";
        assert_ne!(task_spec_hash(original), task_spec_hash(&changed));
        assert_eq!(task_spec_hash(original), task_spec_hash(reordered));
    }

    #[test]
    fn fenced_heading_lines_are_preserved_as_code() {
        let text =
            "## What to build\n```md\n## Notes\nKeep the sample\n```\n## Notes\nIgnore the log\n";
        let normalized = normalized_task_spec(text).unwrap();
        assert!(normalized.contains("## Notes\nKeep the sample"));
        assert!(!normalized.contains("Ignore the log"));
    }

    #[test]
    fn no_spec_heading_returns_json_null() {
        assert_eq!(task_spec_hash("## Notes\nA log."), None);
        assert_eq!(
            handle_request(json!({"text": "No headings"})),
            Ok(Value::Null)
        );
    }

    #[test]
    fn rejects_request_without_text_string() {
        let error = handle_request(json!({"text": 4})).unwrap_err();
        assert_eq!(error.kind, agentmesh_ops_kit::ErrorKind::InvalidRequest);
    }
}

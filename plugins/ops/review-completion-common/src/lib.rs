//! Offline preflight over a synthetic issue/PR snapshot and precomputed helper facts.
//! A passing subject is not a verified receipt and grants no delivery authority.

use agentmesh_ops_kit::OpsError;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub receipt: Value,
    pub context: Context,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Context {
    pub source: Issue,
    pub producer: Issue,
    pub project_id: Option<String>,
    pub project_status: Option<String>,
    pub producer_runs: Vec<Run>,
    pub source_runs: Vec<Run>,
    pub owner_allowlist: Vec<String>,
    pub default_branch: Option<String>,
    pub pr: PullRequest,
    pub helpers: HelperFacts,
}

#[derive(Debug, Default, Deserialize)]
pub struct Issue {
    pub id: Option<String>,
    pub project_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Run {
    pub id: Option<String>,
    pub status: Option<String>,
    pub started_at: Option<String>,
    pub created_at: Option<String>,
}

impl Run {
    fn active(&self) -> bool {
        matches!(
            self.status.as_deref(),
            Some("running" | "queued" | "pending" | "in_progress")
        )
    }

    fn timestamp(&self) -> &str {
        self.started_at
            .as_deref()
            .filter(|value| !value.is_empty())
            .or(self.created_at.as_deref())
            .unwrap_or_default()
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct PullRequest {
    #[serde(rename = "headRefOid")]
    pub head: Option<String>,
    pub state: Option<String>,
    #[serde(rename = "isDraft", default)]
    pub draft: Value,
    #[serde(rename = "baseRefName")]
    pub base: Option<String>,
    pub author: Option<Author>,
}

#[derive(Debug, Deserialize)]
pub struct Author {
    pub login: Option<String>,
}

/// Results of Python's metadata, safety, and URL helpers; not wire assertions
/// that can replace observations of native state.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct HelperFacts {
    pub source_safety_reason: Option<String>,
    pub producer_safety_reason: Option<String>,
    pub producer_metadata: ProducerMetadata,
    pub receipt_url: ReceiptUrl,
    pub canonical_pr_url: Option<String>,
    pub canonical_link_urls: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ProducerMetadata {
    pub task_kind: Option<String>,
    pub source_issue_id: Option<String>,
    pub dedupe_key: String,
    pub todo_runner_ignore_terminal_run_id: Option<String>,
    pub canonical_source_pr_url: Option<String>,
}

/// Parsed components and normalized identities, not a precomputed scope decision.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ReceiptUrl {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
    pub pull_number_is_digits: bool,
    pub canonical_path_repo: String,
    pub canonical_context_repo: Option<String>,
    pub canonical_url: String,
}

fn managed_child(metadata: &ProducerMetadata, source_id: &str) -> bool {
    metadata.task_kind.as_deref() == Some("review_fix")
        && metadata.source_issue_id.as_deref() == Some(source_id)
        && metadata
            .dedupe_key
            .starts_with(&format!("managed-review-fix:{source_id}:"))
}

fn rejection(receipt: &Value, ctx: &Context) -> Option<String> {
    let reject = |reason: &str| Some(reason.to_owned());
    if receipt["schema_version"].as_i64() != Some(1) {
        return reject("completion_receipt_missing");
    }
    let source_id = receipt["source_issue_id"]
        .as_str()
        .filter(|s| !s.is_empty());
    let producer_id = receipt["producer_issue_id"]
        .as_str()
        .filter(|s| !s.is_empty());
    let (Some(source_id), Some(producer_id)) = (source_id, producer_id) else {
        return reject("completion_subject_mismatch");
    };
    if ctx.source.id.as_deref() != Some(source_id)
        || ctx.producer.id.as_deref() != Some(producer_id)
    {
        return reject("completion_subject_mismatch");
    }
    let project_id = ctx.project_id.as_deref().filter(|id| !id.is_empty());
    if project_id.is_none()
        || ctx.source.project_id.as_deref() != project_id
        || ctx.producer.project_id.as_deref() != project_id
        || receipt["project_id"].as_str() != project_id
        || !matches!(
            ctx.project_status.as_deref(),
            Some("planned" | "in_progress")
        )
    {
        return reject("completion_project_mismatch");
    }
    let helpers = &ctx.helpers;
    for reason in [
        &helpers.source_safety_reason,
        &helpers.producer_safety_reason,
    ] {
        if let Some(reason) = reason.as_deref().filter(|r| !r.is_empty()) {
            return Some(format!("completion_protected:{reason}"));
        }
    }
    if ctx.source.status.as_deref() != Some("in_review")
        || !matches!(ctx.producer.status.as_deref(), Some("in_review" | "done"))
    {
        return reject("completion_source_not_reviewable");
    }
    let child = producer_id != source_id;
    let metadata = &helpers.producer_metadata;
    if child && !managed_child(metadata, source_id) {
        return reject("completion_source_contract_mismatch");
    }

    // Python max retains the first run on a tie; Rust max_by_key retains the last.
    let latest = ctx
        .producer_runs
        .iter()
        .rev()
        .max_by_key(|run| run.timestamp());
    if ctx.producer_runs.iter().any(Run::active)
        || !latest.is_some_and(|run| {
            let id = run.id.as_deref().filter(|id| !id.is_empty());
            run.status.as_deref() == Some("completed")
                && id.is_some()
                && id == receipt["producer_run_id"].as_str()
                && id != metadata.todo_runner_ignore_terminal_run_id.as_deref()
        })
    {
        return reject("completion_producer_not_completed");
    }
    if ctx.source_runs.iter().any(Run::active) {
        return reject("completion_source_still_active");
    }

    let url = &helpers.receipt_url;
    let allowed = |login: &str| ctx.owner_allowlist.iter().any(|owner| owner == login);
    if url.scheme != "https"
        || url.netloc != "github.com"
        || !url.query.is_empty()
        || !url.fragment.is_empty()
        || !matches!(url.path.trim_matches('/').split('/').collect::<Vec<_>>().as_slice(),
            [owner, _, "pull", _] if allowed(owner))
        || !url.pull_number_is_digits
        || url.canonical_context_repo.as_deref() != Some(url.canonical_path_repo.as_str())
    {
        return reject("completion_pr_outside_resource");
    }
    let pr = &ctx.pr;
    if !receipt["head_sha"].as_str().is_some_and(|head| {
        head.len() == 40
            && head
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && pr.head.as_deref() == Some(head)
    }) {
        return reject("completion_head_changed");
    }
    let canonical_url = url.canonical_url.as_str();
    if helpers.canonical_pr_url.as_deref() != Some(canonical_url)
        || !matches!(pr.state.as_deref(), Some("OPEN" | "MERGED"))
        || pr.draft.as_bool() != Some(false)
        || pr.base != ctx.default_branch
        || !pr
            .author
            .as_ref()
            .and_then(|author| author.login.as_deref())
            .is_some_and(allowed)
    {
        return reject("completion_pr_scope_changed");
    }
    if !helpers
        .canonical_link_urls
        .iter()
        .any(|link| link == canonical_url)
    {
        return reject("completion_pr_link_missing");
    }
    if helpers
        .canonical_link_urls
        .iter()
        .any(|link| link != canonical_url)
    {
        return reject("completion_pr_ambiguous");
    }
    if child && metadata.canonical_source_pr_url.as_deref() != Some(canonical_url) {
        return reject("completion_parent_pr_mismatch");
    }
    None
}

pub fn completion_subject_plan(request: &Request) -> Value {
    json!({"reason": rejection(&request.receipt, &request.context), "metadata": {}})
}

pub fn handle(input: Value) -> Result<Value, OpsError> {
    // Serde's diagnostic may include values from the rejected request.
    let request = serde_json::from_value(input)
        .map_err(|_| OpsError::invalid_request("invalid subject request"))?;
    Ok(completion_subject_plan(&request))
}

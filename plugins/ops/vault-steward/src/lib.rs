//! Pure port of `vault_steward.build_result`. The caller supplies the root
//! text, plans, and observed results; no operation is planned or executed here.

use agentmesh_ops_kit::OpsError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const REQUIRED_BRANCH: &str = "main";
type Record = Map<String, Value>;

/// Precomputed bucket fields, in caller-supplied order.
#[derive(Debug, Deserialize)]
pub struct BucketPlan {
    pub bucket_id: String,
    pub files: Vec<String>,
    pub message: String,
}

/// Inputs to the result shaper. Commit, sanitization and skip records are
/// passed through, including any caller-supplied keys or null values.
#[derive(Debug, Deserialize)]
pub struct Request {
    pub root: String,
    pub branch: String,
    pub plans: Vec<BucketPlan>,
    pub excluded: Vec<String>,
    pub commits: Vec<Record>,
    pub pushed: bool,
    pub push_target: String,
    pub skipped_reason: String,
    pub trigger: String,
    pub dry_run: bool,
    #[serde(default)]
    pub push_deferred: bool,
    #[serde(default)]
    pub error: String,
    pub conflicted_paths: Option<Vec<String>>,
    pub agents_md_sanitize: Option<Record>,
    pub skipped_buckets: Option<Vec<Record>>,
}

/// Shape the result without interpreting the supplied plan or outcome.
pub fn build_result(request: Request) -> Value {
    fn nonempty(text: &str) -> Option<&str> {
        (!text.is_empty()).then_some(text)
    }

    let buckets: Vec<_> = request
        .plans
        .into_iter()
        .map(|plan| {
            json!({
                "id": plan.bucket_id,
                "files": plan.files,
                "file_count": plan.files.len(),
                "message": plan.message,
            })
        })
        .collect();

    let mut result = json!({
        "branch": request.branch,
        "required_branch": REQUIRED_BRANCH,
        "dry_run": request.dry_run,
        "trigger": nonempty(&request.trigger),
        "agents_md_sanitize": request.agents_md_sanitize,
        "buckets": buckets,
        "excluded": request.excluded,
        "commits": request.commits,
        "commit_count": request.commits.len(),
        "skipped_buckets": request.skipped_buckets.unwrap_or_default(),
        "pushed": request.pushed,
        "push_deferred": request.push_deferred,
        "push_target": nonempty(&request.push_target),
        "skipped_reason": nonempty(&request.skipped_reason),
        "error": nonempty(&request.error),
        "vault_root": request.root,
    });
    if let Some(paths) = request.conflicted_paths.filter(|paths| !paths.is_empty()) {
        result["conflicted_paths"] = json!(paths);
    }
    result
}

/// Process one JSON request for the ops-kit stdio adapter.
pub fn handle(input: Value) -> Result<Value, OpsError> {
    if !input.is_object() {
        return Err(OpsError::invalid_request("request must be a JSON object"));
    }
    serde_json::from_value(input)
        .map(build_result)
        .map_err(|_| OpsError::invalid_request("invalid vault stewardship result inputs"))
}

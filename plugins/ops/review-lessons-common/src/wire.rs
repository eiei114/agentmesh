//! Preserve numeric ID tokens without changing serde_json number semantics
//! for other workspace crates (Cargo unifies dependency features).

use std::collections::BTreeMap;

use agentmesh_ops_kit::{parse_request, OpsError};
use serde_json::{value::RawValue, Value};

use crate::{build_index, handle, NumericId, NumericIds, Object};

type RawObject<'a> = BTreeMap<String, &'a RawValue>;

pub(super) fn numeric_id(token: &str) -> Result<Option<NumericId<'_>>, OpsError> {
    if !matches!(token.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
        return Ok(None);
    }
    if token.contains(['.', 'e', 'E']) {
        token
            .parse()
            .map(|f| Some(NumericId::Float(f)))
            .map_err(|_| OpsError::invalid_request("invalid floating-point identifier"))
    } else {
        Ok(Some(NumericId::Integer(token)))
    }
}

fn parse_record(raw: &RawValue) -> Result<(Value, NumericIds<'_>), OpsError> {
    let fields: RawObject<'_> = serde_json::from_str(raw.get())
        .map_err(|_| OpsError::invalid_request("records must contain objects"))?;
    let mut record = Object::new();
    let mut ids = NumericIds::default();
    for (key, raw) in fields {
        let slot = match key.as_str() {
            "finding_url" => Some(&mut ids.finding_url),
            "comment_id" => Some(&mut ids.comment_id),
            "evidence_id" => Some(&mut ids.evidence_id),
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = numeric_id(raw.get())?;
            if slot.is_some() {
                // The sidecar is authoritative for numeric IDs. In particular,
                // do not expose a numeric finding_url as a string to the URL aggregator.
                continue;
            }
        }
        record.insert(key, parse_request(raw.get())?);
    }
    Ok((Value::Object(record), ids))
}

/// Handle original JSON text, keeping integers in ID fields exact regardless of size.
/// Prefer this over converting incoming JSON to `Value` first.
pub fn handle_json(input: &str) -> Result<Value, OpsError> {
    let mut fields: RawObject<'_> = serde_json::from_str(input).map_err(|err| {
        OpsError::invalid_request(format!("request must be a JSON object: {err}"))
    })?;
    if !fields.get("op").is_some_and(|op| {
        serde_json::from_str::<String>(op.get()).ok().as_deref() == Some("build_index")
    }) {
        return handle(parse_request(input)?);
    }
    let records = fields
        .remove("records")
        .and_then(|raw| serde_json::from_str::<Vec<&RawValue>>(raw.get()).ok())
        .ok_or_else(|| OpsError::invalid_request("records must be an array"))?;
    let (records, ids): (Vec<_>, Vec<_>) = records
        .into_iter()
        .map(parse_record)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .unzip();
    let mut request: Object = fields
        .into_iter()
        .map(|(key, raw)| Ok((key, parse_request(raw.get())?)))
        .collect::<Result<_, OpsError>>()?;
    request.insert("records".into(), Value::Array(records));
    build_index(&Value::Object(request), &ids)
}

//! Offline cohort scoring for already-parsed benchmark snapshots.

use std::collections::BTreeMap;

use agentmesh_ops_kit::OpsError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

pub const PRIOR: f64 = 0.25;
// Keep Python's accumulation order. The source field is the snapshot key.
pub const WEIGHTS: [(&str, f64); 9] = [
    ("epoch_mirrorcode", 0.20),
    ("epoch_swe_verified", 0.15),
    ("aider_polyglot", 0.10),
    ("arena_agent", 0.25),
    ("arena_webdev", 0.20),
    ("epoch_gpqa", 0.05),
    ("vendor_deepswe", 0.03),
    ("vendor_swe_verified", 0.01),
    ("hf_lightning_swe", 0.01),
];
pub const VENDOR_METRICS: [&str; 3] = ["vendor_deepswe", "vendor_swe_verified", "hf_lightning_swe"];
const SOURCES: [(&str, &[&str]); 6] = [
    (
        "epoch",
        &["epoch_mirrorcode", "epoch_swe_verified", "epoch_gpqa"],
    ),
    ("aider", &["aider_polyglot"]),
    ("arena_agent", &["arena_agent"]),
    ("arena_webdev", &["arena_webdev"]),
    ("llm_stats", &["vendor_deepswe", "vendor_swe_verified"]),
    ("hf_lightning", &["hf_lightning_swe"]),
];

type Object = Map<String, Value>;
type Rows = Vec<Object>;
type Result<T> = std::result::Result<T, OpsError>;

fn invalid(message: &'static str) -> OpsError {
    OpsError::invalid_request(message)
}

fn decimal_digit(c: char) -> Option<char> {
    // Non-ASCII decimal zero code points (Unicode 16.0, as in Python 3.14).
    const ZERO: &[u32] = &[
        0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66, 0xde6,
        0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
        0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0,
        0xff10, 0x104a0, 0x10d30, 0x10d40, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450,
        0x114d0, 0x11650, 0x116c0, 0x116d0, 0x116da, 0x11730, 0x118e0, 0x11950, 0x11bf0, 0x11c50,
        0x11d50, 0x11da0, 0x11f50, 0x16130, 0x16a60, 0x16ac0, 0x16b50, 0x16d70, 0x1ccf0, 0x1d7ce,
        0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e4f0, 0x1e5f1, 0x1e950, 0x1fbf0,
    ];
    let code = u32::from(c);
    let index = ZERO.partition_point(|&zero| zero <= code).checked_sub(1)?;
    char::from_digit(code - ZERO[index], 10)
}

fn parse_score(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_ascii() && !text.contains('_') {
        return text.parse().ok();
    }
    let normalized: String = text
        .chars()
        .map(|c| {
            if c.is_ascii() {
                c
            } else {
                decimal_digit(c).unwrap_or(c)
            }
        })
        .collect();
    let bytes = normalized.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_'
            && !(i > 0
                && bytes[i - 1].is_ascii_digit()
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            return None;
        }
    }
    normalized.replace('_', "").parse().ok()
}

pub fn number(value: &Value, low: Option<f64>, high: Option<f64>) -> Result<f64> {
    let score = value
        .as_f64()
        .or_else(|| value.as_str().and_then(parse_score))
        .ok_or_else(|| invalid("missing/non-numeric score"))?;
    if !score.is_finite() || low.is_some_and(|v| score < v) || high.is_some_and(|v| score > v) {
        return Err(invalid("out-of-range score"));
    }
    Ok(score)
}

fn row_id(row: &Object) -> Result<&str> {
    row.get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
                .is_empty()
        })
        .ok_or_else(|| invalid("invalid benchmark ID"))
}

fn medians(rows: &[Object]) -> Result<Vec<(&str, &Object, f64, usize)>> {
    let mut grouped = BTreeMap::new();
    for row in rows {
        let id = row_id(row)?;
        let score = number(row.get("score").unwrap_or(&Value::Null), None, None)?;
        let (_, scores) = grouped.entry(id).or_insert_with(|| (row, Vec::new()));
        scores.push(score);
    }
    grouped
        .into_iter()
        .map(|(id, (first, mut scores))| {
            // Stable sort preserves Python's treatment of equal scores, including +/-0.
            scores.sort_by(|a, b| a.partial_cmp(b).expect("scores are finite"));
            let n = scores.len();
            let mid = n / 2;
            let median = if n % 2 == 0 {
                (scores[mid - 1] + scores[mid]) / 2.0
            } else {
                scores[mid]
            };
            if !median.is_finite() {
                return Err(invalid("median is not finite"));
            }
            Ok((id, first, median, n))
        })
        .collect()
}

pub fn collapse(rows: &[Object]) -> Result<Rows> {
    Ok(medians(rows)?
        .into_iter()
        .map(|(_, first, median, n)| {
            let mut row = first.clone();
            row.insert("score".into(), json!(median));
            row.insert("measurements".into(), json!(n));
            row
        })
        .collect())
}

pub fn percentiles(entries: &[Object]) -> Result<BTreeMap<&str, f64>> {
    let mut scores: Vec<_> = medians(entries)?
        .into_iter()
        .map(|(id, _, score, _)| (id, score))
        .collect();
    scores.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).expect("scores are finite"));
    let n = scores.len();
    let mut result = BTreeMap::new();
    let mut start = 0;
    while start < n {
        let end = start + scores[start..].partition_point(|entry| entry.1 == scores[start].1);
        let percentile = if n == 1 {
            0.5
        } else {
            (start as f64 + (end - start - 1) as f64 / 2.0) / (n - 1) as f64
        };
        for &(id, _) in &scores[start..end] {
            result.insert(id, percentile);
        }
        start = end;
    }
    Ok(result)
}

#[derive(Deserialize)]
pub struct Source {
    #[serde(default)]
    metrics: BTreeMap<String, Rows>,
    stale: bool,
}

#[derive(Deserialize)]
pub struct Aliases {
    #[serde(default)]
    models: BTreeMap<String, Option<Alias>>,
}

#[derive(Deserialize)]
struct Alias {
    #[serde(default)]
    identity: Value,
    #[serde(default)]
    metadata: Object,
    #[serde(default)]
    entries: BTreeMap<String, Option<String>>,
}

fn integer(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
        .or_else(|| value.as_bool().map(i128::from))
}

// Python compares JSON metadata structurally, and int/bool/float by numeric value.
fn metadata_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| metadata_eq(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| metadata_eq(a, b)))
        }
        _ => match (integer(a), integer(b)) {
            (Some(a), Some(b)) => a == b,
            (Some(integer), None) => b
                .as_f64()
                .is_some_and(|f| f.fract() == 0.0 && f as i128 == integer),
            (None, Some(integer)) => a
                .as_f64()
                .is_some_and(|f| f.fract() == 0.0 && f as i128 == integer),
            (None, None) => a == b || a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)| a == b),
        },
    }
}

pub fn match_model(
    model: &str,
    metadata: &Object,
    aliases: &Aliases,
    sources: &BTreeMap<String, Source>,
) -> Result<Value> {
    let alias = aliases.models.get(model).and_then(Option::as_ref);
    let identity = alias.map_or(&Value::Null, |a| &a.identity);
    let reason = match alias {
        None => Some("alias_missing"),
        Some(_) if identity.is_null() => Some("identity_unverified"),
        Some(a)
            if a.metadata
                .iter()
                .any(|(k, v)| !metadata_eq(metadata.get(k).unwrap_or(&Value::Null), v)) =>
        {
            Some("metadata_mismatch")
        }
        Some(_) => None,
    };
    if sources.len() != SOURCES.len() {
        return Err(invalid("supply all six benchmark source records"));
    }
    let mut matches = Object::new();
    let mut normalized = BTreeMap::new();
    for (name, metrics) in SOURCES {
        let source = sources
            .get(name)
            .ok_or_else(|| invalid("supply all six benchmark source records"))?;
        for &metric in metrics {
            let vendor = VENDOR_METRICS.contains(&metric);
            let entry_id = alias
                .and_then(|a| a.entries.get(metric))
                .and_then(Option::as_deref);
            let rows = source.metrics.get(metric).map_or(&[][..], Vec::as_slice);
            let evidence_kind = if vendor {
                "vendor-reported"
            } else {
                "independent"
            };
            let outcome = if let Some(reason) = reason {
                Err(reason)
            } else if let Some(id) = entry_id.filter(|id| !id.is_empty()) {
                if rows.is_empty() {
                    Err("source_unavailable")
                } else {
                    rows.iter()
                        .find(|r| r.get("id").and_then(Value::as_str) == Some(id))
                        .map(|r| (id, r))
                        .ok_or("no_entry")
                }
            } else {
                Err("no_alias")
            };
            let result = match outcome {
                Ok((id, entry)) => {
                    let raw = entry
                        .get("score")
                        .ok_or_else(|| invalid("missing/non-numeric score"))?;
                    let (percentile, value) = if vendor {
                        (
                            None,
                            raw.as_f64()
                                .ok_or_else(|| invalid("vendor score must be a number"))?
                                / 100.0,
                        )
                    } else {
                        let percentile = percentiles(rows)?[id];
                        (Some(percentile), percentile)
                    };
                    normalized.insert(metric, value);
                    json!({"status":"matched", "entry_id": entry_id, "raw": raw,
                        "percentile": percentile, "normalized": value, "source": name,
                        "stale": source.stale, "evidence_kind": evidence_kind,
                        "original_source": entry.get("original_source"),
                        "observed_at": entry.get("observed_at"), "release_date": entry.get("release_date")})
                }
                Err(status) => json!({"status": status, "entry_id": entry_id, "raw": null,
                    "percentile": null, "normalized": null, "source": name,
                    "stale": source.stale, "evidence_kind": evidence_kind}),
            };
            matches.insert(metric.into(), result);
        }
    }
    // Neumaier compensation matches Python 3.12+'s float sum.
    let (mut sum, mut correction) = (0.0_f64, 0.0_f64);
    for (metric, weight) in WEIGHTS {
        let term = weight * normalized.get(metric).copied().unwrap_or(PRIOR);
        let next = sum + term;
        correction += if sum.abs() >= term.abs() {
            (sum - next) + term
        } else {
            (term - next) + sum
        };
        sum = next;
    }
    Ok(
        json!({"status": reason.unwrap_or(if normalized.is_empty() { "unmatched" } else { "matched" }),
        "identity": identity, "sources": matches, "quality": sum + correction}),
    )
}

fn zero() -> Option<f64> {
    Some(0.0)
}
fn hundred() -> Option<f64> {
    Some(100.0)
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Number {
        value: Value,
        #[serde(default = "zero")]
        low: Option<f64>,
        #[serde(default = "hundred")]
        high: Option<f64>,
    },
    Collapse {
        rows: Rows,
    },
    Percentiles {
        entries: Rows,
    },
    MatchModel {
        model: String,
        metadata: Object,
        aliases: Aliases,
        sources: BTreeMap<String, Source>,
    },
}

pub fn handle(request: Value) -> Result<Value> {
    let request = serde_json::from_value(request)
        .map_err(|_| invalid("invalid benchmark request shape or op"))?;
    match request {
        Request::Number { value, low, high } => Ok(json!(number(&value, low, high)?)),
        Request::Collapse { rows } => Ok(json!(collapse(&rows)?)),
        Request::Percentiles { entries } => Ok(json!(percentiles(&entries)?)),
        Request::MatchModel {
            model,
            metadata,
            aliases,
            sources,
        } => match_model(&model, &metadata, &aliases, &sources),
    }
}

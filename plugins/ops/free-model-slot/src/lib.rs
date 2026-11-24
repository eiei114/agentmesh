//! Read-only ranking of a caller-supplied free-model snapshot.

use std::cmp::Ordering;

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Number, Value};

const EXTERNAL_V1: &str = "external-benchmarks-v1";
const EXTERNAL_V2: &str = "external-benchmarks-v2";
const EXTERNAL_GATES: [&str; 5] = [
    "free",
    "available",
    "not_deprecated",
    "tool_metadata",
    "not_exhausted",
];
const LEGACY_GATES: [&str; 5] = ["free", "available", "not_deprecated", "smoke", "tools"];

/// Both base prices and every additional cost must be numeric zero, not false.
pub fn zero_cost(meta: &Value) -> bool {
    meta["cost"].as_object().is_some_and(|cost| {
        cost.contains_key("input")
            && cost.contains_key("output")
            && cost.values().all(|value| value.as_f64() == Some(0.0))
    })
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

/// External snapshots require exactly the external gates; legacy snapshots
/// require exactly the legacy gates and at least two successful evaluations.
pub fn passing(row: &Value) -> bool {
    let external = matches!(
        row["ranking_method"].as_str(),
        Some(EXTERNAL_V1 | EXTERNAL_V2)
    );
    let required = if external {
        EXTERNAL_GATES
    } else {
        LEGACY_GATES
    };
    let gates_match = row["gates"].as_object().is_some_and(|gates| {
        gates.len() == required.len()
            && required.iter().all(|gate| {
                gates.get(*gate).is_some_and(|value| {
                    // Python dictionary equality treats numeric 1 as equal to True.
                    value.as_bool() == Some(true) || value.as_f64() == Some(1.0)
                })
            })
    });
    gates_match
        && (external
            || row["evals"].as_array().is_some_and(|evals| {
                evals.len() >= 2 && evals.iter().all(|eval| truthy(&eval["passed"]))
            }))
}

#[derive(Clone, Copy)]
enum Numeric {
    Integer(i128),
    Float(f64),
}

impl Numeric {
    fn negative(value: &Value, field: &str) -> Result<Self, OpsError> {
        if let Some(value) = value.as_i64() {
            Ok(Self::Integer(-i128::from(value)))
        } else if let Some(value) = value.as_u64() {
            Ok(Self::Integer(-i128::from(value)))
        } else if let Some(value) = value.as_f64() {
            Ok(Self::Float(-value))
        } else if let Some(value) = value.as_bool() {
            Ok(Self::Integer(-i128::from(value)))
        } else {
            Err(OpsError::invalid_request(format!(
                "{field} must be numeric"
            )))
        }
    }

    fn cmp(self, other: Self) -> Ordering {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a.cmp(&b),
            (Self::Float(a), Self::Float(b)) => compare_finite(a, b),
            (Self::Integer(a), Self::Float(b)) => compare_integer_float(a, b),
            (Self::Float(a), Self::Integer(b)) => compare_integer_float(b, a).reverse(),
        }
    }

    fn json(self) -> Value {
        match self {
            Self::Integer(value) => {
                Number::from_i128(value).map_or_else(|| json!(value as f64), Value::Number)
            }
            Self::Float(value) => json!(value),
        }
    }
}

fn compare_finite(a: f64, b: f64) -> Ordering {
    // Unlike total_cmp, Python treats -0.0 and 0.0 as equal. JSON numbers are finite.
    a.partial_cmp(&b).expect("finite JSON numbers")
}

fn compare_integer_float(integer: i128, float: f64) -> Ordering {
    // Compare the integer part without rounding the integer to f64. The
    // saturating cast is safe for ordering: our integers are at most u64 wide.
    integer
        .cmp(&(float as i128))
        .then_with(|| compare_finite(0.0, float.fract()))
}

/// The sortable Python tuple: three negated metrics followed by model ID.
pub struct RankingKey<'a> {
    metrics: [Numeric; 3],
    model: &'a str,
}

impl RankingKey<'_> {
    pub fn compare(&self, other: &Self) -> Ordering {
        self.metrics[0]
            .cmp(other.metrics[0])
            .then_with(|| self.metrics[1].cmp(other.metrics[1]))
            .then_with(|| self.metrics[2].cmp(other.metrics[2]))
            .then_with(|| self.model.cmp(other.model))
    }

    fn json(&self) -> Value {
        let [first, second, third] = self.metrics.map(Numeric::json);
        json!([first, second, third, self.model])
    }
}

fn object_field<'a>(value: &'a Value, field: &str) -> Result<Option<&'a Value>, OpsError> {
    match value.get(field) {
        Some(value) if !value.is_object() => Err(OpsError::invalid_request(format!(
            "{field} must be an object"
        ))),
        value => Ok(value),
    }
}

fn metric(object: Option<&Value>, field: &str, default: Numeric) -> Result<Numeric, OpsError> {
    object
        .and_then(|object| object.get(field))
        .map_or(Ok(default), |value| Numeric::negative(value, field))
}

/// V2 uses quality, completions and uncapped context. Older methods use score.
pub fn ranking_key(row: &Value) -> Result<RankingKey<'_>, OpsError> {
    let model = row["model"]
        .as_str()
        .ok_or_else(|| OpsError::invalid_request("model must be a string"))?;
    let zero = Numeric::Integer(0);
    let metrics = if row["ranking_method"] == EXTERNAL_V2 {
        let limit = object_field(row, "metadata")?
            .map(|metadata| object_field(metadata, "limit"))
            .transpose()?
            .flatten();
        [
            metric(
                object_field(row, "benchmark")?,
                "quality",
                Numeric::Float(-0.25),
            )?,
            metric(object_field(row, "real_runs")?, "completed", zero)?,
            metric(limit, "context", zero)?,
        ]
    } else {
        [Numeric::negative(&row["score"], "score")?, zero, zero]
    };
    Ok(RankingKey { metrics, model })
}

/// Return the original passing rows in stable key order. Failing rows need no key.
pub fn winners(rows: &[Value]) -> Result<Vec<&Value>, OpsError> {
    let mut ranked = Vec::new();
    for row in rows {
        if !row.is_object() {
            return Err(OpsError::invalid_request("models must contain objects"));
        }
        if passing(row) {
            ranked.push((ranking_key(row)?, row));
        }
    }
    ranked.sort_by(|(a, _), (b, _)| a.compare(b));
    Ok(ranked.into_iter().map(|(_, row)| row).collect())
}

/// Dispatch a single JSON request without consulting any external state.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let object = |field: &str| {
        request
            .get(field)
            .filter(|value| value.is_object())
            .ok_or_else(|| OpsError::invalid_request(format!("{field} must be an object")))
    };
    match request["op"].as_str() {
        Some("zero_cost") => Ok(json!(zero_cost(object("meta")?))),
        Some("passing") => Ok(json!(passing(object("row")?))),
        Some("ranking_key") => Ok(ranking_key(object("row")?)?.json()),
        Some("winners") => {
            let rows = request["models"]
                .as_array()
                .ok_or_else(|| OpsError::invalid_request("models must be an array"))?;
            Ok(json!(winners(rows)?))
        }
        _ => Err(OpsError::invalid_request("unknown ranking operation")),
    }
}

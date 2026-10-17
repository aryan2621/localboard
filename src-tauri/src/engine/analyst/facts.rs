//! Numbers are computed here, in code; the model only puts them into words.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::engine::query::{ColumnKind, QueryResult};

const PREVIEW_ROWS: usize = 10;
const MIN_ANOMALY_POINTS: usize = 8;
const Z_SCORE_LIMIT: f64 = 3.0;
const IQR_FENCE: f64 = 1.5;
const MAX_ANOMALIES: usize = 5;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnomalyRule {
    ZScore,
    Iqr,
    Both,
}

/// A point of a time series far from the rest of it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anomaly {
    pub period: Value,
    pub measure: String,
    pub value: f64,
    pub z_score: f64,
    pub rule: AnomalyRule,
}

#[derive(Clone, Serialize)]
pub struct ColumnSummary {
    pub column: String,
    pub total: f64,
    pub min: f64,
    pub max: f64,
    pub average: f64,
}

#[derive(Clone, Serialize)]
pub struct Trend {
    pub axis: String,
    pub measure: String,
    pub first: (Value, f64),
    pub last: (Value, f64),
    pub change: f64,
    pub change_pct: Option<f64>,
}

#[derive(Clone, Serialize)]
pub struct Extremes {
    pub label: String,
    pub measure: String,
    pub highest: (Value, f64),
    pub lowest: (Value, f64),
}

#[derive(Clone, Serialize)]
pub struct Facts {
    pub row_count: usize,
    pub truncated: bool,
    pub preview: Vec<Map<String, Value>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub summaries: Vec<ColumnSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trend: Option<Trend>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extremes: Option<Extremes>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub anomalies: Vec<Anomaly>,
}

pub fn round(value: f64) -> f64 {
    if value.abs() >= 100.0 { (value * 100.0).round() / 100.0 } else { (value * 10_000.0).round() / 10_000.0 }
}

pub fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn column_values(result: &QueryResult, index: usize) -> Vec<f64> {
    result.rows.iter().filter_map(|row| row.get(index).and_then(as_f64)).collect()
}

fn pairs(result: &QueryResult, axis: usize, measure: usize) -> Vec<(Value, f64)> {
    result.rows.iter().filter_map(|row| Some((row.get(axis)?.clone(), as_f64(row.get(measure)?)?))).collect()
}

/// The first temporal column and the first numeric column after it, if the result is a time series.
fn time_series(result: &QueryResult) -> Option<(usize, usize)> {
    let axis = result.columns.iter().position(|column| column.kind == ColumnKind::Temporal)?;
    let measure = result.columns.iter().enumerate().position(|(index, column)| index != axis && column.kind == ColumnKind::Number)?;
    Some((axis, measure))
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    let position = q * (sorted.len() - 1) as f64;
    let (low, high) = (position.floor() as usize, position.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (position - low as f64)
}

/// Flags points with |z| ≥ 3 or outside the 1.5 × IQR fences. Only for a single series with one
/// row per period; several series mixed together would make the statistics meaningless.
fn anomalies(result: &QueryResult) -> Vec<Anomaly> {
    let Some((axis, measure)) = time_series(result) else { return Vec::new() };
    let points = pairs(result, axis, measure);
    let mut periods = HashSet::new();
    if points.len() < MIN_ANOMALY_POINTS || !points.iter().all(|(period, _)| periods.insert(period.to_string())) {
        return Vec::new();
    }
    let values: Vec<f64> = points.iter().map(|(_, value)| *value).collect();
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let deviation = (values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / (count - 1.0)).sqrt();
    let mut sorted = values;
    sorted.sort_by(f64::total_cmp);
    let (q1, q3) = (quantile(&sorted, 0.25), quantile(&sorted, 0.75));
    let spread = q3 - q1;
    let (low, high) = (q1 - IQR_FENCE * spread, q3 + IQR_FENCE * spread);

    let mut found: Vec<Anomaly> = points
        .into_iter()
        .filter_map(|(period, value)| {
            let z_score = if deviation > 0.0 { (value - mean) / deviation } else { 0.0 };
            let by_z = z_score.abs() >= Z_SCORE_LIMIT;
            let by_iqr = spread > 0.0 && (value < low || value > high);
            let rule = match (by_z, by_iqr) {
                (true, true) => AnomalyRule::Both,
                (true, false) => AnomalyRule::ZScore,
                (false, true) => AnomalyRule::Iqr,
                (false, false) => return None,
            };
            Some(Anomaly { period, measure: result.columns[measure].name.clone(), value: round(value), z_score: round(z_score), rule })
        })
        .collect();
    found.sort_by(|a, b| b.z_score.abs().total_cmp(&a.z_score.abs()));
    found.truncate(MAX_ANOMALIES);
    found
}

pub fn compute(result: &QueryResult) -> Facts {
    let columns = &result.columns;
    let preview = result
        .rows
        .iter()
        .take(PREVIEW_ROWS)
        .map(|row| columns.iter().zip(row).map(|(column, value)| (column.name.clone(), value.clone())).collect())
        .collect();

    let numeric: Vec<usize> =
        columns.iter().enumerate().filter(|(_, column)| column.kind == ColumnKind::Number).map(|(index, _)| index).collect();
    let summaries = if result.rows.len() > 1 {
        numeric
            .iter()
            .filter_map(|&index| {
                let values = column_values(result, index);
                if values.is_empty() {
                    return None;
                }
                let total: f64 = values.iter().sum();
                Some(ColumnSummary {
                    column: columns[index].name.clone(),
                    total: round(total),
                    min: round(values.iter().copied().fold(f64::INFINITY, f64::min)),
                    max: round(values.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                    average: round(total / values.len() as f64),
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let axis_of = |kind: ColumnKind| columns.iter().position(|column| column.kind == kind);
    let first_measure = |axis: usize| numeric.iter().copied().find(|&index| index != axis);

    let trend = time_series(result).and_then(|(axis, measure)| {
        let mut points = pairs(result, axis, measure);
        if points.len() < 2 {
            return None;
        }
        points.sort_by(|a, b| a.0.as_str().unwrap_or_default().cmp(b.0.as_str().unwrap_or_default()));
        let (first, last) = (points.first()?.clone(), points.last()?.clone());
        let change = last.1 - first.1;
        Some(Trend {
            axis: columns[axis].name.clone(),
            measure: columns[measure].name.clone(),
            change: round(change),
            change_pct: (first.1 != 0.0).then(|| round(100.0 * change / first.1.abs())),
            first: (first.0, round(first.1)),
            last: (last.0, round(last.1)),
        })
    });

    let extremes = axis_of(ColumnKind::Text).and_then(|label| {
        let measure = first_measure(label)?;
        let points = pairs(result, label, measure);
        if points.len() < 2 {
            return None;
        }
        let highest = points.iter().cloned().max_by(|a, b| a.1.total_cmp(&b.1))?;
        let lowest = points.iter().cloned().min_by(|a, b| a.1.total_cmp(&b.1))?;
        Some(Extremes {
            label: columns[label].name.clone(),
            measure: columns[measure].name.clone(),
            highest: (highest.0, round(highest.1)),
            lowest: (lowest.0, round(lowest.1)),
        })
    });

    Facts { row_count: result.rows.len(), truncated: result.truncated, preview, summaries, trend, extremes, anomalies: anomalies(result) }
}

/// Cheap checks for answers that are probably wrong. Each warning is shown to the user and,
/// when any fire, the model is asked to double-check its query.
pub fn sanity_warnings(sql: &str, result: &QueryResult, largest_source_rows: Option<i64>) -> Vec<String> {
    let mut warnings = Vec::new();
    if result.rows.is_empty() {
        warnings.push("The query returned no rows; a filter may not match the data.".to_owned());
        return warnings;
    }
    for (index, column) in result.columns.iter().enumerate() {
        if column.kind == ColumnKind::Number && result.rows.iter().all(|row| row.get(index).is_none_or(Value::is_null)) {
            warnings.push(format!("\"{}\" is empty in every row.", column.name));
        }
    }
    let lowered = sql.to_lowercase();
    if lowered.contains(" join ") {
        if let Some(largest) = largest_source_rows {
            if result.rows.len() as i64 > largest && !result.truncated {
                warnings.push("The result has more rows than any source table; a join may be duplicating rows.".to_owned());
            }
        }
    }
    if lowered.contains("group by") {
        let dimensions: Vec<usize> = result
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.kind != ColumnKind::Number)
            .map(|(index, _)| index)
            .collect();
        if !dimensions.is_empty() {
            let mut seen = HashSet::new();
            let repeated = result.rows.iter().any(|row| {
                let key: Vec<String> = dimensions.iter().map(|&index| row.get(index).map(Value::to_string).unwrap_or_default()).collect();
                !seen.insert(key)
            });
            if repeated {
                warnings.push("Some groups appear more than once; the grouping may be at the wrong level.".to_owned());
            }
        }
    }
    warnings
}

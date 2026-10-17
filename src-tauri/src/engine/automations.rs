//! Deterministic automation facts and English alerts. The model only narrates these numbers.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::engine::analyst::facts::{self, as_f64, Anomaly, Facts};
use crate::engine::query::QueryResult;

const STEP_KINDS: &[&str] = &["sql", "ask", "dashboard", "compare", "threshold", "anomaly", "report", "email", "mcp_tool", "webhook", "write_file", "notify", "branch"];
const TRIGGER_KINDS: &[&str] = &["cron", "interval", "file_change", "manual", "webhook"];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Threshold {
    pub column: Option<String>,
    pub op: String,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub text: String,
    pub facts: Value,
    pub hash: String,
    pub fired: bool,
}

pub fn valid_step(kind: &str) -> bool {
    STEP_KINDS.contains(&kind)
}

pub fn valid_trigger(kind: &str) -> bool {
    TRIGGER_KINDS.contains(&kind)
}

pub fn interval_secs(trigger: &Value) -> Option<i64> {
    trigger.get("intervalSecs").and_then(Value::as_i64).filter(|secs| *secs >= 60)
}

pub fn cron_expr(trigger: &Value) -> Option<&str> {
    trigger.get("cron").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())
}

pub fn watch_path(trigger: &Value) -> Option<&str> {
    trigger
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// Accepts 5-field (min hour day month weekday) or 6-field (with seconds) cron.
pub fn normalize_cron(expr: &str) -> Result<String, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    let with_seconds = match fields.len() {
        5 => format!("0 {expr}"),
        6 => expr.to_owned(),
        _ => return Err("Cron uses five fields (minute hour day month weekday) or six with seconds".into()),
    };
    with_seconds.parse::<cron::Schedule>().map_err(|error| error.to_string())?;
    Ok(with_seconds)
}

pub fn next_cron(expr: &str) -> Result<std::time::Duration, String> {
    let schedule: cron::Schedule = normalize_cron(expr)?.parse().map_err(|error: cron::error::Error| error.to_string())?;
    let next = schedule.upcoming(chrono::Local).next().ok_or_else(|| "That cron expression never fires".to_owned())?;
    let wait = (next - chrono::Local::now()).to_std().unwrap_or(std::time::Duration::from_secs(1));
    Ok(wait.max(std::time::Duration::from_secs(1)))
}

pub fn sql_facts(result: &QueryResult) -> (Facts, Value, String) {
    let facts = facts::compute(result);
    let value = serde_json::to_value(&facts).unwrap_or(Value::Null);
    let hash = crate::retrieval::content_hash(&serde_json::to_string(&result.rows).unwrap_or_default());
    (facts, value, hash)
}

fn metric(facts: &Facts, column: Option<&str>) -> Option<(String, f64)> {
    if let Some(name) = column.map(str::trim).filter(|text| !text.is_empty()) {
        if let Some(summary) = facts.summaries.iter().find(|item| item.column.eq_ignore_ascii_case(name)) {
            return Some((summary.column.clone(), summary.total));
        }
        if let Some(row) = facts.preview.first() {
            if let Some(value) = row.get(name).and_then(as_f64) {
                return Some((name.to_owned(), facts::round(value)));
            }
        }
        return None;
    }
    facts.summaries.first().map(|item| (item.column.clone(), item.total)).or_else(|| {
        facts.trend.as_ref().map(|trend| (trend.measure.clone(), trend.last.1))
    })
}

pub fn compare_alert(current: &Facts, previous: Option<&Value>, column: Option<&str>) -> Alert {
    let Some((name, now)) = metric(current, column) else {
        return Alert {
            text: "There is no numeric column to compare with the previous run.".into(),
            facts: json!({ "rowCount": current.row_count }),
            hash: crate::retrieval::content_hash("compare-empty"),
            fired: false,
        };
    };
    let before = previous.and_then(|value| {
        value.get("metric").and_then(|metric| metric.get("value").and_then(as_f64)).or_else(|| {
            value.get("summaries").and_then(Value::as_array).and_then(|items| {
                items.iter().find(|item| item.get("column").and_then(Value::as_str).is_some_and(|column| column.eq_ignore_ascii_case(&name)))
                    .and_then(|item| item.get("total").and_then(as_f64))
            })
        })
    });
    let facts = json!({
        "column": name,
        "current": now,
        "previous": before,
        "change": before.map(|was| facts::round(now - was)),
        "changePct": before.filter(|was| *was != 0.0).map(|was| facts::round(100.0 * (now - was) / was.abs())),
        "rowCount": current.row_count,
        "metric": { "column": name, "value": now },
        "summaries": current.summaries,
    });
    let text = match before {
        Some(was) if (now - was).abs() < f64::EPSILON => {
            format!("{name} is still {now}, unchanged from the previous run.")
        }
        Some(was) => {
            let change = facts::round(now - was);
            let direction = if change > 0.0 { "up" } else { "down" };
            match facts.get("changePct").and_then(as_f64) {
                Some(pct) => format!("{name} was {was} last run and is {now} now, {direction} {} ({pct}%).", change.abs()),
                None => format!("{name} was {was} last run and is {now} now, {direction} {}.", change.abs()),
            }
        }
        None => format!("{name} is {now}. There is no previous run to compare."),
    };
    let fired = before.is_some_and(|was| (now - was).abs() > f64::EPSILON);
    Alert { hash: crate::retrieval::content_hash(&text), text, facts, fired }
}

pub fn threshold_alert(current: &Facts, spec: &Threshold) -> Alert {
    let Some((name, now)) = metric(current, spec.column.as_deref()) else {
        return Alert {
            text: "There is no numeric column to check against the threshold.".into(),
            facts: json!({ "rowCount": current.row_count }),
            hash: crate::retrieval::content_hash("threshold-empty"),
            fired: false,
        };
    };
    let (fired, relation) = match spec.op.as_str() {
        "gt" | ">" => (now > spec.value, "above"),
        "gte" | ">=" => (now >= spec.value, "at or above"),
        "lt" | "<" => (now < spec.value, "below"),
        "lte" | "<=" => (now <= spec.value, "at or below"),
        "eq" | "=" => ((now - spec.value).abs() < f64::EPSILON, "equal to"),
        "neq" | "!=" => ((now - spec.value).abs() >= f64::EPSILON, "not equal to"),
        _ => (false, "compared to"),
    };
    let facts = json!({
        "column": name,
        "current": now,
        "threshold": spec.value,
        "op": spec.op,
        "crossed": fired,
        "metric": { "column": name, "value": now },
        "summaries": current.summaries,
    });
    let text = if fired {
        format!("{name} is {now}, which is {relation} the threshold of {}.", spec.value)
    } else {
        format!("{name} is {now}, which has not crossed the threshold of {}.", spec.value)
    };
    Alert { hash: crate::retrieval::content_hash(&text), text, facts, fired }
}

pub fn anomaly_alert(current: &Facts) -> Alert {
    let anomalies: &[Anomaly] = &current.anomalies;
    let facts = json!({
        "rowCount": current.row_count,
        "anomalies": anomalies,
        "trend": current.trend,
        "summaries": current.summaries,
        "metric": current.summaries.first().map(|item| json!({ "column": item.column, "value": item.total })),
    });
    let text = if anomalies.is_empty() {
        if current.row_count < 8 {
            "Not enough periods to judge unusual values (need at least 8).".to_owned()
        } else {
            "No unusual periods (z-score ≥ 3 or outside 1.5 × IQR).".to_owned()
        }
    } else {
        let first = &anomalies[0];
        let extra = if anomalies.len() > 1 { format!(" {} more unusual period(s) were flagged.", anomalies.len() - 1) } else { String::new() };
        format!(
            "{} {} is unusual at {} (z-score {}, {}).{extra}",
            first.measure,
            first.period,
            first.value,
            first.z_score,
            match first.rule {
                facts::AnomalyRule::Both => "z-score and IQR",
                facts::AnomalyRule::ZScore => "z-score",
                facts::AnomalyRule::Iqr => "IQR",
            }
        )
    };
    Alert { fired: !anomalies.is_empty(), hash: crate::retrieval::content_hash(&text), text, facts }
}

pub fn sql_alert(result: &QueryResult) -> Alert {
    let (computed, facts, hash) = sql_facts(result);
    let text = match computed.trend {
        Some(trend) => format!(
            "{} went from {} to {} ({}, {} rows).",
            trend.measure,
            trend.first.1,
            trend.last.1,
            trend.change,
            computed.row_count
        ),
        None if !computed.summaries.is_empty() => {
            let first = &computed.summaries[0];
            format!("{} totals {} across {} rows.", first.column, first.total, computed.row_count)
        }
        None => format!("{} rows.", computed.row_count),
    };
    Alert { text, facts, hash, fired: true }
}

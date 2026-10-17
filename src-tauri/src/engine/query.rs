//! Executes guard-validated SQL with a timeout, a row cap and cancellation. Every run happens
//! on its own cloned DuckDB connection inside a transaction that is always rolled back.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use duckdb::arrow::datatypes::DataType;
use duckdb::types::{TimeUnit, Value as DuckValue};
use duckdb::{Connection, InterruptHandle};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use tokio::sync::Notify;

use crate::sql_guard::ValidatedQuery;

pub const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_ROWS: usize = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnKind {
    Number,
    Temporal,
    Text,
    Boolean,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultColumn {
    pub name: String,
    pub data_type: String,
    pub kind: ColumnKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<ResultColumn>,
    pub rows: Vec<Vec<Value>>,
    /// More rows existed than the cap allowed.
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("The query took longer than {} seconds and was stopped.", QUERY_TIMEOUT.as_secs())]
    TimedOut,
    #[error("Stopped.")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// Lets a running analysis be stopped: wakes async waiters and interrupts any running query.
#[derive(Default)]
pub struct CancelToken {
    cancelled: AtomicBool,
    notify: Notify,
    running_query: Mutex<Option<Arc<InterruptHandle>>>,
}

impl CancelToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
        if let Ok(running) = self.running_query.lock() {
            if let Some(handle) = running.as_ref() {
                handle.interrupt();
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Resolves once `cancel` has been called.
    pub async fn cancelled(&self) {
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }

    fn track(&self, handle: Option<Arc<InterruptHandle>>) {
        if let Ok(mut running) = self.running_query.lock() {
            *running = handle;
        }
    }
}

pub fn run(
    conn: &Connection,
    query: &ValidatedQuery,
    params: &HashMap<String, DuckValue>,
    cancel: &CancelToken,
) -> Result<QueryResult, QueryError> {
    if cancel.is_cancelled() {
        return Err(QueryError::Cancelled);
    }
    crate::engine::mask::authorize_query(query).map_err(|error| QueryError::Failed(error.to_string()))?;
    for name in &query.parameters {
        if !params.contains_key(name) {
            return Err(QueryError::Failed(format!("Set a value for :{name}.")));
        }
    }
    let bound: HashMap<String, DuckValue> =
        query.parameters.iter().filter_map(|name| params.get(name).map(|value| (name.clone(), value.clone()))).collect();
    let rendered = duck_placeholders(&query.sql);
    let sql = if query.has_limit { rendered } else { format!("{rendered} LIMIT {}", MAX_ROWS + 1) };

    let handle = conn.interrupt_handle();
    cancel.track(Some(handle.clone()));
    let timed_out = Arc::new(AtomicBool::new(false));
    let (finished, wait) = mpsc::channel::<()>();
    let timer = {
        let timed_out = timed_out.clone();
        std::thread::spawn(move || {
            if wait.recv_timeout(QUERY_TIMEOUT) == Err(mpsc::RecvTimeoutError::Timeout) {
                timed_out.store(true, Ordering::SeqCst);
                handle.interrupt();
            }
        })
    };

    let outcome = execute(conn, &sql, &bound);
    drop(finished);
    let _ = timer.join();
    cancel.track(None);

    let mut result = outcome.map_err(|error| {
        if timed_out.load(Ordering::SeqCst) {
            QueryError::TimedOut
        } else if cancel.is_cancelled() {
            QueryError::Cancelled
        } else {
            QueryError::Failed(error.to_string())
        }
    })?;
    crate::engine::mask::finish(&mut result, &query.relations);
    Ok(result)
}

fn execute(conn: &Connection, sql: &str, params: &HashMap<String, DuckValue>) -> Result<QueryResult, duckdb::Error> {
    conn.execute_batch("BEGIN TRANSACTION;")?;
    let result = read(conn, sql, params);
    if let Err(error) = conn.execute_batch("ROLLBACK;") {
        tracing::warn!("rolling back a read-only query transaction failed: {error}");
    }
    result
}

/// `:name` is what people type. DuckDB binds `$name`. Values are never interpolated.
/// `::` casts, quoted strings/identifiers and comments are left untouched.
pub fn duck_placeholders(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' || ch == '"' {
            out.push(ch);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == ch {
                    if chars.get(i + 1) == Some(&ch) {
                        i += 1;
                        out.push(ch);
                    } else {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            continue;
        }
        if ch == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if ch == '/' && chars.get(i + 1) == Some(&'*') {
            out.push_str("/*");
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                out.push_str("*/");
                i += 2;
            }
            continue;
        }
        if ch == ':' {
            if chars.get(i + 1) == Some(&':') {
                out.push_str("::");
                i += 2;
                continue;
            }
            let starts = chars.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == '_');
            if starts {
                out.push('$');
            } else {
                out.push(':');
            }
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

pub fn bind_params(names: &[String], raw: &serde_json::Map<String, Value>) -> Result<HashMap<String, DuckValue>, String> {
    let mut bound = HashMap::new();
    for name in names {
        let value = raw.get(name).ok_or_else(|| format!("Set a value for :{name}."))?;
        bound.insert(name.clone(), json_to_duck(value)?);
    }
    Ok(bound)
}

fn json_to_duck(value: &Value) -> Result<DuckValue, String> {
    match value {
        Value::Null => Ok(DuckValue::Null),
        Value::Bool(flag) => Ok(DuckValue::Boolean(*flag)),
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(DuckValue::BigInt(integer))
            } else {
                number.as_f64().map(DuckValue::Double).ok_or_else(|| format!("{number} is not a number DuckDB can bind."))
            }
        }
        Value::String(text) => Ok(coerce_text(text)),
        other => Err(format!("Parameter values are text, numbers, booleans or null, not {other}.")),
    }
}

fn coerce_text(text: &str) -> DuckValue {
    if let Ok(integer) = text.parse::<i64>() {
        if integer.to_string() == text {
            return DuckValue::BigInt(integer);
        }
    }
    if let Ok(number) = text.parse::<f64>() {
        if text.contains('.') && number.is_finite() {
            return DuckValue::Double(number);
        }
    }
    DuckValue::Text(text.to_owned())
}

fn read(conn: &Connection, sql: &str, params: &HashMap<String, DuckValue>) -> Result<QueryResult, duckdb::Error> {
    let mut statement = conn.prepare(sql)?;
    let mut rows = statement.query(params)?;
    let columns = match rows.as_ref() {
        Some(statement) => statement
            .column_names()
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let data_type = statement.column_type(index);
                ResultColumn { name, kind: kind_of(&data_type), data_type: data_type.to_string() }
            })
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    let mut values = Vec::new();
    let mut truncated = false;
    while let Some(row) = rows.next()? {
        if values.len() == MAX_ROWS {
            truncated = true;
            break;
        }
        let mut record = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            record.push(to_json(row.get::<_, DuckValue>(index)?));
        }
        values.push(record);
    }
    Ok(QueryResult { columns, rows: values, truncated })
}

fn kind_of(data_type: &DataType) -> ColumnKind {
    match data_type {
        DataType::Boolean => ColumnKind::Boolean,
        DataType::Date32 | DataType::Date64 | DataType::Timestamp(..) => ColumnKind::Temporal,
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View | DataType::Dictionary(..) => ColumnKind::Text,
        other if other.is_numeric() => ColumnKind::Number,
        _ => ColumnKind::Other,
    }
}

fn number(value: f64) -> Value {
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// Integers that JavaScript cannot represent exactly are sent as strings.
fn wide_integer(value: i128) -> Value {
    const SAFE: i128 = 9_007_199_254_740_991;
    if (-SAFE..=SAFE).contains(&value) {
        Value::Number(Number::from(value as i64))
    } else {
        Value::String(value.to_string())
    }
}

pub fn to_json(value: DuckValue) -> Value {
    match value {
        DuckValue::Null => Value::Null,
        DuckValue::Boolean(flag) => Value::Bool(flag),
        DuckValue::TinyInt(v) => Value::from(v),
        DuckValue::SmallInt(v) => Value::from(v),
        DuckValue::Int(v) => Value::from(v),
        DuckValue::BigInt(v) => wide_integer(i128::from(v)),
        DuckValue::HugeInt(v) => wide_integer(v),
        DuckValue::UTinyInt(v) => Value::from(v),
        DuckValue::USmallInt(v) => Value::from(v),
        DuckValue::UInt(v) => Value::from(v),
        DuckValue::UBigInt(v) => wide_integer(i128::from(v)),
        DuckValue::UHugeInt(v) => i128::try_from(v).map_or_else(|_| Value::String(v.to_string()), wide_integer),
        DuckValue::Float(v) => number(f64::from(v)),
        DuckValue::Double(v) => number(v),
        DuckValue::Decimal(v) => v.to_string().parse::<f64>().map_or_else(|_| Value::String(v.to_string()), number),
        DuckValue::Timestamp(unit, v) => Value::String(format_timestamp(unit, v)),
        DuckValue::Date32(days) => Value::String(format_date(i64::from(days))),
        DuckValue::Time64(unit, v) => Value::String(format_time(unit.to_micros(v))),
        DuckValue::Interval { months, days, nanos } => {
            Value::String(format!("{months} months {days} days {}s", nanos as f64 / 1e9))
        }
        DuckValue::Text(text) | DuckValue::Enum(text) => Value::String(text),
        DuckValue::Blob(bytes) | DuckValue::Geometry(bytes) => Value::String(format!("<{} bytes>", bytes.len())),
        DuckValue::List(items) | DuckValue::Array(items) => Value::Array(items.into_iter().map(to_json).collect()),
        DuckValue::Struct(fields) => {
            Value::Object(fields.iter().map(|(key, value)| (key.clone(), to_json(value.clone()))).collect())
        }
        DuckValue::Map(entries) => Value::Array(
            entries
                .iter()
                .map(|(key, value)| Value::Array(vec![to_json(key.clone()), to_json(value.clone())]))
                .collect(),
        ),
        DuckValue::Union(inner) => to_json(*inner),
        other => Value::String(format!("{other:?}")),
    }
}

/// Days since 1970-01-01 to `YYYY-MM-DD` (proleptic Gregorian; Howard Hinnant's civil_from_days).
fn format_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn format_time(micros_of_day: i64) -> String {
    let seconds = micros_of_day.div_euclid(1_000_000);
    let micros = micros_of_day.rem_euclid(1_000_000);
    let base = format!("{:02}:{:02}:{:02}", seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if micros == 0 { base } else { format!("{base}.{micros:06}") }
}

fn format_timestamp(unit: TimeUnit, value: i64) -> String {
    const MICROS_PER_DAY: i64 = 86_400_000_000;
    let micros = unit.to_micros(value);
    let date = format_date(micros.div_euclid(MICROS_PER_DAY));
    let time = micros.rem_euclid(MICROS_PER_DAY);
    if time == 0 { date } else { format!("{date} {}", format_time(time)) }
}

/// Milliseconds since `started`, saturating, for storage.
pub fn elapsed_ms(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

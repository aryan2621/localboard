use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use duckdb::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::duck::{quote_ident, sql_string};

const SAMPLE_ROWS: usize = 5;
const SAMPLE_VALUES: usize = 3;
const TOP_VALUES: usize = 5;
const SNAPSHOT_ROWS: usize = 25_000;
const PROFILE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueCount {
    pub value: String,
    pub count: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnProfile {
    pub name: String,
    pub data_type: String,
    pub null_pct: f64,
    pub distinct_count: i64,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub sample_values: Vec<String>,
    pub top_values: Vec<ValueCount>,
}

impl ColumnProfile {
    /// Every non-null value is distinct, so the column can serve as a join key.
    pub fn is_unique(&self, row_count: i64) -> bool {
        let non_null = row_count as f64 * (1.0 - self.null_pct / 100.0);
        row_count > 0 && self.null_pct == 0.0 && (self.distinct_count as f64 - non_null).abs() < 0.5
    }
}

pub type SampleRow = BTreeMap<String, Value>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableProfile {
    pub row_count: i64,
    pub columns: Vec<ColumnProfile>,
    pub samples: Vec<SampleRow>,
}

/// Copies up to [`SNAPSHOT_ROWS`] into a temp table (one CSV sniff), then profiles that table.
/// Interrupted after [`PROFILE_TIMEOUT`] so a stuck read cannot pin the catalog lock.
pub fn profile(conn: &Connection, relation: &str) -> Result<TableProfile, duckdb::Error> {
    with_timeout(conn, || profile_inner(conn, relation))
}

fn profile_inner(conn: &Connection, relation: &str) -> Result<TableProfile, duckdb::Error> {
    let table = quote_ident(relation);
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS __lb_profile; CREATE TEMP TABLE __lb_profile AS SELECT * FROM {table} LIMIT {SNAPSHOT_ROWS};"
    ))?;
    let result = profile_table(conn, &table, "__lb_profile");
    let _ = conn.execute_batch("DROP TABLE IF EXISTS __lb_profile;");
    result
}

/// Row count, null share and min/max come from the full relation; distinct counts are exact up to
/// the snapshot size and approximate beyond it. Top and sample values come from the snapshot.
fn profile_table(conn: &Connection, full: &str, snapshot: &str) -> Result<TableProfile, duckdb::Error> {
    let table = quote_ident(snapshot);
    let columns_meta = describe(conn, &table)?;
    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {full}"), [], |row| row.get(0))?;
    let (row_count, stats) = column_stats(conn, full, &columns_meta, total > SNAPSHOT_ROWS as i64)?;
    let tops = if row_count == 0 || columns_meta.is_empty() {
        vec![Vec::new(); columns_meta.len()]
    } else {
        top_values(conn, &table, &columns_meta)?
    };

    let mut columns = Vec::with_capacity(columns_meta.len());
    for (index, (name, data_type)) in columns_meta.iter().enumerate() {
        let (nulls, distinct_count, min_value, max_value) = stats[index].clone();
        let top = tops.get(index).cloned().unwrap_or_default();
        columns.push(ColumnProfile {
            name: name.clone(),
            data_type: data_type.clone(),
            null_pct: if row_count == 0 { 0.0 } else { 100.0 * nulls as f64 / row_count as f64 },
            distinct_count,
            min_value,
            max_value,
            sample_values: top.iter().take(SAMPLE_VALUES).map(|entry| entry.value.clone()).collect(),
            top_values: top,
        });
    }

    Ok(TableProfile { row_count, samples: samples(conn, &table, &columns_meta)?, columns })
}

type ColumnStats = (i64, i64, Option<String>, Option<String>);

fn column_stats(conn: &Connection, table: &str, columns: &[(String, String)], approximate: bool) -> Result<(i64, Vec<ColumnStats>), duckdb::Error> {
    if columns.is_empty() {
        let row_count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))?;
        return Ok((row_count, Vec::new()));
    }
    let mut select = vec!["COUNT(*)".to_string()];
    for (name, _) in columns {
        let column = quote_ident(name);
        select.push(format!("COUNT(*) - COUNT({column})"));
        select.push(if approximate { format!("approx_count_distinct({column})") } else { format!("COUNT(DISTINCT {column})") });
        select.push(format!("CAST(MIN({column}) AS VARCHAR)"));
        select.push(format!("CAST(MAX({column}) AS VARCHAR)"));
    }
    conn.query_row(&format!("SELECT {} FROM {table}", select.join(", ")), [], |row| {
        let row_count: i64 = row.get(0)?;
        let mut stats = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            let base = 1 + index * 4;
            stats.push((row.get(base)?, row.get(base + 1)?, row.get(base + 2)?, row.get(base + 3)?));
        }
        Ok((row_count, stats))
    })
}

fn top_values(conn: &Connection, table: &str, columns: &[(String, String)]) -> Result<Vec<Vec<ValueCount>>, duckdb::Error> {
    let union = columns
        .iter()
        .map(|(name, _)| {
            let column = quote_ident(name);
            format!(
                "(SELECT {} AS name, CAST({column} AS VARCHAR) AS value, COUNT(*) AS cnt FROM {table} WHERE {column} IS NOT NULL GROUP BY 2 ORDER BY 3 DESC, 2 LIMIT {TOP_VALUES})",
                sql_string(name)
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let mut statement = conn.prepare(&format!("SELECT name, value, cnt FROM ({union}) AS tops"))?;
    let mut grouped = vec![Vec::new(); columns.len()];
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
    })?;
    for row in rows {
        let (name, value, count) = row?;
        if let Some(index) = columns.iter().position(|(column, _)| column == &name) {
            grouped[index].push(ValueCount { value, count });
        }
    }
    Ok(grouped)
}

fn describe(conn: &Connection, table: &str) -> Result<Vec<(String, String)>, duckdb::Error> {
    let mut statement = conn.prepare(&format!("DESCRIBE SELECT * FROM {table}"))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    rows.collect()
}

fn samples(conn: &Connection, table: &str, columns: &[(String, String)]) -> Result<Vec<SampleRow>, duckdb::Error> {
    if columns.is_empty() {
        return Ok(Vec::new());
    }
    let projection = columns
        .iter()
        .map(|(name, _)| format!("CAST({} AS VARCHAR)", quote_ident(name)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut statement = conn.prepare(&format!("SELECT {projection} FROM {table} LIMIT {SAMPLE_ROWS}"))?;
    let rows = statement.query_map([], |row| {
        let mut record = BTreeMap::new();
        for (index, (name, _)) in columns.iter().enumerate() {
            let value: Option<String> = row.get(index)?;
            record.insert(name.clone(), value.map(Value::String).unwrap_or(Value::Null));
        }
        Ok(record)
    })?;
    rows.collect()
}

fn with_timeout<T>(conn: &Connection, work: impl FnOnce() -> Result<T, duckdb::Error>) -> Result<T, duckdb::Error> {
    let handle = conn.interrupt_handle();
    let timed_out = Arc::new(AtomicBool::new(false));
    let (finished, wait) = mpsc::channel::<()>();
    let timer = {
        let timed_out = timed_out.clone();
        std::thread::spawn(move || {
            if wait.recv_timeout(PROFILE_TIMEOUT) == Err(mpsc::RecvTimeoutError::Timeout) {
                timed_out.store(true, Ordering::SeqCst);
                handle.interrupt();
            }
        })
    };
    let result = work();
    drop(finished);
    let _ = timer.join();
    match result {
        Err(error) if timed_out.load(Ordering::SeqCst) => Err(duckdb::Error::QueryReturnedNoRows),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_a_small_table_in_one_stats_pass() {
        let conn = Connection::open_in_memory().expect("memory duckdb");
        conn.execute_batch(
            "CREATE TABLE customers (customer_id INTEGER, region VARCHAR);
             INSERT INTO customers VALUES (1, 'West'), (2, 'East'), (3, 'West'), (4, NULL);",
        )
        .expect("seed");
        let profiled = profile(&conn, "customers").expect("profile");
        assert_eq!(profiled.row_count, 4);
        assert_eq!(profiled.columns.len(), 2);
        let region = profiled.columns.iter().find(|column| column.name == "region").expect("region");
        assert_eq!(region.distinct_count, 2);
        assert!((region.null_pct - 25.0).abs() < 0.01);
        assert_eq!(region.top_values[0].value, "West");
        assert_eq!(region.top_values[0].count, 2);
    }

    #[test]
    fn profiles_a_csv_view() {
        let path = std::env::temp_dir().join("localboard_profile_test.csv");
        std::fs::write(&path, "customer_id,region\n1,West\n2,East\n3,West\n").expect("write csv");
        let conn = Connection::open_in_memory().expect("memory duckdb");
        conn.execute_batch(&format!(
            "CREATE VIEW customers AS SELECT * FROM read_csv_auto({});",
            sql_string(&path.to_string_lossy())
        ))
        .expect("view");
        let profiled = profile(&conn, "customers").expect("profile");
        assert_eq!(profiled.row_count, 3);
        let _ = std::fs::remove_file(path);
    }
}

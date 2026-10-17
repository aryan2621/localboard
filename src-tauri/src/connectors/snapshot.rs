//! Read-only snapshot imports: remote rows land in a DuckDB table with an explicit "as of" stamp.

use std::collections::HashMap;

use super::{ConnectorError, Snapshot};
use crate::engine::duck::{quote_ident, sql_string, Analytics};

const MARKER: &str = "lb-snapshot";

#[derive(Clone, Debug)]
pub struct SnapshotInfo {
    pub taken_at: String,
    #[allow(dead_code)]
    pub rows: i64,
    pub truncated: bool,
}

/// Replaces `relation` with a typed copy of `snapshot` and stamps it with the time of the copy.
pub fn load(duck: &Analytics, relation: &str, snapshot: &Snapshot) -> Result<(), ConnectorError> {
    let raw = format!("{relation}__raw");
    let raw_quoted = quote_ident(&raw);
    let mut columns = Vec::with_capacity(snapshot.columns.len());
    for (index, name) in snapshot.columns.iter().enumerate() {
        let mut name = if name.trim().is_empty() { format!("column_{}", index + 1) } else { name.clone() };
        while columns.contains(&name) {
            name.push('_');
        }
        columns.push(name);
    }
    duck.replace_text_table(&raw, &columns, snapshot.rows.iter().cloned())?;
    let conn = duck.connection();
    let outcome = (|| -> Result<(), duckdb::Error> {
        let projection = columns
            .iter()
            .map(|name| infer(conn, &raw_quoted, name))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        duck.drop_relation(relation)?;
        conn.execute_batch(&format!("CREATE TABLE {} AS SELECT {projection} FROM {raw_quoted};", quote_ident(relation)))?;
        let comment = format!(
            "{MARKER}|{}|{}|{}",
            chrono::Utc::now().to_rfc3339(),
            snapshot.rows.len(),
            u8::from(snapshot.truncated)
        );
        conn.execute_batch(&format!("COMMENT ON TABLE {} IS {};", quote_ident(relation), sql_string(&comment)))
    })();
    let _ = conn.execute_batch(&format!("DROP TABLE IF EXISTS {raw_quoted};"));
    outcome.map_err(ConnectorError::from)
}

/// Picks BIGINT, DOUBLE, TIMESTAMP or leaves VARCHAR, whichever every non-null value converts to.
fn infer(conn: &duckdb::Connection, table: &str, column: &str) -> Result<String, duckdb::Error> {
    let quoted = quote_ident(column);
    let (total, ints, doubles, stamps): (i64, i64, i64, i64) = conn.query_row(
        &format!(
            "SELECT count({quoted}), count(TRY_CAST({quoted} AS BIGINT)), count(TRY_CAST({quoted} AS DOUBLE)), count(TRY_CAST({quoted} AS TIMESTAMP)) FROM {table}"
        ),
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let target = if total == 0 {
        None
    } else if ints == total {
        Some("BIGINT")
    } else if doubles == total {
        Some("DOUBLE")
    } else if stamps == total {
        Some("TIMESTAMP")
    } else {
        None
    };
    Ok(match target {
        Some(kind) => format!("CAST({quoted} AS {kind}) AS {quoted}"),
        None => quoted,
    })
}

/// Snapshot stamps for the given relations; relations that are live views are absent.
pub fn infos(duck: &Analytics, relations: &[String]) -> HashMap<String, SnapshotInfo> {
    let mut found = HashMap::new();
    let Ok(mut statement) = duck.connection().prepare(&format!(
        "SELECT table_name, comment FROM duckdb_tables() WHERE schema_name = 'main' AND comment LIKE '{MARKER}|%'"
    )) else {
        return found;
    };
    let Ok(rows) = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))) else {
        return found;
    };
    for (name, comment) in rows.flatten() {
        if !relations.contains(&name) {
            continue;
        }
        let parts: Vec<&str> = comment.split('|').collect();
        if let [_, taken_at, rows, truncated] = parts[..] {
            found.insert(
                name,
                SnapshotInfo { taken_at: taken_at.to_owned(), rows: rows.parse().unwrap_or(0), truncated: truncated == "1" },
            );
        }
    }
    found
}

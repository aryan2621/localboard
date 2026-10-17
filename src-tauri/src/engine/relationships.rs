use duckdb::Connection;

use super::duck::quote_ident;
use super::profile::ColumnProfile;

/// Share of sampled foreign-key values that must exist in the candidate key column.
const MIN_CONTAINMENT: f64 = 0.9;
const CONTAINMENT_SAMPLE: usize = 1000;

pub struct TableInfo<'a> {
    pub id: &'a str,
    pub relation: &'a str,
    pub row_count: i64,
    pub columns: &'a [ColumnProfile],
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetectedRelationship {
    pub from_table_id: String,
    pub from_column: String,
    pub to_table_id: String,
    pub to_column: String,
    pub confidence: f64,
}

/// Finds likely foreign keys: a `<thing>_id` column whose values are contained in a unique key
/// column of a table named after `<thing>`. Name heuristics propose, a data check confirms.
pub fn detect(conn: &Connection, tables: &[TableInfo<'_>]) -> Vec<DetectedRelationship> {
    let mut found = Vec::new();
    for from in tables {
        for column in from.columns {
            let Some(stem) = key_stem(&column.name) else { continue };
            for to in tables.iter().filter(|to| to.id != from.id && names_match(&stem, to.relation)) {
                let Some(key) = key_column(to, &column.name) else { continue };
                match containment(conn, from.relation, &column.name, to.relation, &key.name) {
                    Ok(Some(confidence)) if confidence >= MIN_CONTAINMENT => found.push(DetectedRelationship {
                        from_table_id: from.id.to_owned(),
                        from_column: column.name.clone(),
                        to_table_id: to.id.to_owned(),
                        to_column: key.name.clone(),
                        confidence,
                    }),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(
                        "relationship check {}.{} -> {}.{} failed: {error}",
                        from.relation,
                        column.name,
                        to.relation,
                        key.name
                    ),
                }
            }
        }
    }
    found
}

fn key_stem(column: &str) -> Option<String> {
    let lower = column.to_lowercase();
    if let Some(stem) = lower.strip_suffix("_id") {
        return (!stem.is_empty()).then(|| stem.to_owned());
    }
    // camelCase: customerId
    let stem = column.strip_suffix("Id")?;
    (!stem.is_empty() && stem.chars().last().is_some_and(char::is_lowercase)).then(|| stem.to_lowercase())
}

fn names_match(stem: &str, relation: &str) -> bool {
    let relation = relation.to_lowercase();
    let mut forms = vec![stem.to_owned(), format!("{stem}s"), format!("{stem}es")];
    if let Some(base) = stem.strip_suffix('y') {
        forms.push(format!("{base}ies"));
    }
    forms.iter().any(|form| relation == *form || relation.ends_with(&format!("_{form}")))
}

fn key_column<'a>(table: &'a TableInfo<'_>, fk_name: &str) -> Option<&'a ColumnProfile> {
    table
        .columns
        .iter()
        .filter(|column| column.name.eq_ignore_ascii_case(fk_name) || column.name.eq_ignore_ascii_case("id"))
        .find(|column| column.is_unique(table.row_count))
}

fn containment(conn: &Connection, from: &str, from_column: &str, to: &str, to_column: &str) -> Result<Option<f64>, duckdb::Error> {
    let (from, from_column, to, to_column) = (quote_ident(from), quote_ident(from_column), quote_ident(to), quote_ident(to_column));
    let (total, matched): (i64, i64) = conn.query_row(
        &format!(
            "WITH sampled AS (
               SELECT DISTINCT CAST({from_column} AS VARCHAR) AS v FROM {from}
               WHERE {from_column} IS NOT NULL LIMIT {CONTAINMENT_SAMPLE}
             )
             SELECT COUNT(*), COUNT(keys.k) FROM sampled
             LEFT JOIN (SELECT DISTINCT CAST({to_column} AS VARCHAR) AS k FROM {to}) keys ON sampled.v = keys.k"
        ),
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((total > 0).then(|| matched as f64 / total as f64))
}

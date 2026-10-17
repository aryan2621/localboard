//! Picks the tables relevant to a question and renders them compactly for a small local model.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;

use crate::error::AppResult;
use crate::state::Workspace;
use crate::storage::workspace_db::{ColumnRecord, RelationshipRecord, TableRecord};

const MAX_TABLES: usize = 6;
const MAX_TABLES_WITH_JOINS: usize = 8;
const MAX_COLUMNS: usize = 40;
const SAMPLE_VALUES: usize = 3;
const SAMPLE_CHARS: usize = 40;
const STARTER_QUESTIONS: usize = 4;

/// Untrusted cell values are wrapped in these so the model can tell data from instructions.
pub const DATA_OPEN: char = '«';
pub const DATA_CLOSE: char = '»';

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "what", "which", "who", "how", "many", "much", "show", "give", "list", "last", "this",
    "that", "from", "are", "was", "were", "per", "each", "all", "did", "does", "why", "top", "by", "of", "in", "me",
];

pub struct TableSchema {
    pub relation: String,
    pub columns: Vec<(String, String)>,
}

pub struct SchemaContext {
    pub tables: Vec<TableSchema>,
    pub prompt: String,
}

impl SchemaContext {
    pub fn table(&self, relation: &str) -> Option<&TableSchema> {
        self.tables.iter().find(|table| table.relation.eq_ignore_ascii_case(relation))
    }
}

struct Candidate {
    table: TableRecord,
    columns: Vec<ColumnRecord>,
}

pub(crate) fn tokens(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.len() >= 3 && !STOP_WORDS.contains(&word.as_str()))
        .map(|word| word.strip_suffix('s').filter(|stem| stem.len() >= 3).map(str::to_owned).unwrap_or(word))
        .collect()
}

fn score(question: &HashSet<String>, candidate: &Candidate) -> u32 {
    let hits = |text: &str| tokens(text).intersection(question).count() as u32;
    let mut total = 5 * hits(&candidate.table.relation) + hits(&candidate.table.description);
    for column in &candidate.columns {
        total += 2 * hits(&column.name) + hits(&column.description);
        if let Some(profile) = &column.profile {
            total += 3 * profile.top_values.iter().map(|top| hits(&top.value)).sum::<u32>();
        }
    }
    total
}

fn clip(value: &str) -> String {
    let flat: String = value.chars().filter(|c| *c != DATA_OPEN && *c != DATA_CLOSE && !c.is_control()).collect();
    if flat.chars().count() <= SAMPLE_CHARS {
        flat
    } else {
        format!("{}…", flat.chars().take(SAMPLE_CHARS).collect::<String>())
    }
}

/// Wraps text cells of a result preview in the data fence before they go into a prompt.
pub fn fence_preview(rows: &mut [serde_json::Map<String, serde_json::Value>]) {
    for cell in rows.iter_mut().flat_map(|row| row.values_mut()) {
        if let serde_json::Value::String(text) = cell {
            *text = format!("{DATA_OPEN}{}{DATA_CLOSE}", clip(text));
        }
    }
}

fn render(candidates: &[&Candidate], relationships: &[RelationshipRecord], by_id: &HashMap<&str, &str>) -> String {
    let mut out = String::new();
    for candidate in candidates {
        let table = &candidate.table;
        let rows = table.row_count.map_or_else(|| "unknown rows".to_owned(), |count| format!("{count} rows"));
        let _ = write!(out, "TABLE \"{}\" ({rows})", table.relation);
        if !table.description.is_empty() {
            let _ = write!(out, " -- {}", table.description);
        }
        out.push('\n');
        for column in candidate.columns.iter().take(MAX_COLUMNS) {
            let _ = write!(out, "  \"{}\" {}", column.name, column.data_type);
            if !column.description.is_empty() {
                let _ = write!(out, " -- {}", column.description);
            }
            if let Some(profile) = &column.profile {
                let samples: Vec<String> = profile
                    .sample_values
                    .iter()
                    .take(SAMPLE_VALUES)
                    .map(|value| format!("{DATA_OPEN}{}{DATA_CLOSE}", clip(value)))
                    .collect();
                if !samples.is_empty() {
                    let _ = write!(out, " e.g. {}", samples.join(", "));
                }
            }
            out.push('\n');
        }
        if candidate.columns.len() > MAX_COLUMNS {
            let _ = writeln!(out, "  … {} more columns", candidate.columns.len() - MAX_COLUMNS);
        }
    }
    let included: HashSet<&str> = candidates.iter().map(|candidate| candidate.table.id.as_str()).collect();
    let joins: Vec<String> = relationships
        .iter()
        .filter(|rel| included.contains(rel.from_table_id.as_str()) && included.contains(rel.to_table_id.as_str()))
        .filter_map(|rel| {
            Some(format!(
                "\"{}\".\"{}\" = \"{}\".\"{}\"",
                by_id.get(rel.from_table_id.as_str())?,
                rel.from_column,
                by_id.get(rel.to_table_id.as_str())?,
                rel.to_column
            ))
        })
        .collect();
    if !joins.is_empty() {
        let _ = writeln!(out, "JOINS:\n  {}", joins.join("\n  "));
    }
    out
}

fn load(workspace: &Workspace) -> AppResult<(Vec<Candidate>, Vec<RelationshipRecord>)> {
    let policy = workspace.policy();
    let masks = policy.query_masks();
    let db = workspace.db()?;
    let candidates = db
        .list_tables()?
        .into_iter()
        .filter(|table| table.profile_error.is_none())
        .map(|table| {
            let mut columns = db.columns(&table.id)?;
            // Values from masked columns never reach the model.
            for column in &mut columns {
                let masked = masks.iter().any(|mask| {
                    mask.relation.eq_ignore_ascii_case(&table.relation) && mask.column_name.eq_ignore_ascii_case(&column.name)
                });
                if let (Some(profile), true) = (&mut column.profile, masked) {
                    profile.sample_values.clear();
                    profile.top_values.clear();
                }
            }
            Ok(Candidate { columns, table })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((candidates, db.relationships()?))
}

/// `only` limits the schema to those relations. An empty list means every table.
pub fn build(workspace: &Workspace, question: &str, only: &[String]) -> AppResult<SchemaContext> {
    let (mut candidates, relationships) = load(workspace)?;
    if !only.is_empty() {
        let allowed: HashSet<String> = only.iter().map(|name| name.to_lowercase()).collect();
        let scoped: Vec<_> = candidates.into_iter().filter(|candidate| allowed.contains(&candidate.table.relation.to_lowercase())).collect();
        if scoped.is_empty() {
            return Err(crate::error::AppError::invalid(
                "This analyst is limited to tables that are not connected. Assign tables that exist, or clear the assignment.",
            ));
        }
        candidates = scoped;
    }
    let wanted = tokens(question);
    let mut ranked: Vec<(u32, &Candidate)> = candidates.iter().map(|candidate| (score(&wanted, candidate), candidate)).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.table.row_count.cmp(&a.1.table.row_count)));

    let mut chosen: Vec<&Candidate> = ranked.iter().take(MAX_TABLES).map(|(_, candidate)| *candidate).collect();
    let mut ids: BTreeSet<&str> = chosen.iter().map(|candidate| candidate.table.id.as_str()).collect();
    for rel in &relationships {
        if chosen.len() >= MAX_TABLES_WITH_JOINS {
            break;
        }
        let partner = if ids.contains(rel.from_table_id.as_str()) {
            &rel.to_table_id
        } else if ids.contains(rel.to_table_id.as_str()) {
            &rel.from_table_id
        } else {
            continue;
        };
        if let Some(candidate) = candidates.iter().find(|candidate| &candidate.table.id == partner) {
            if ids.insert(candidate.table.id.as_str()) {
                chosen.push(candidate);
            }
        }
    }

    let by_id: HashMap<&str, &str> =
        candidates.iter().map(|candidate| (candidate.table.id.as_str(), candidate.table.relation.as_str())).collect();
    Ok(SchemaContext {
        prompt: render(&chosen, &relationships, &by_id),
        tables: chosen
            .iter()
            .map(|candidate| TableSchema {
                relation: candidate.table.relation.clone(),
                columns: candidate.columns.iter().map(|column| (column.name.clone(), column.data_type.clone())).collect(),
            })
            .collect(),
    })
}

fn is_numeric_type(data_type: &str) -> bool {
    let upper = data_type.to_uppercase();
    ["INT", "DOUBLE", "DECIMAL", "FLOAT", "REAL", "NUMERIC"].iter().any(|marker| upper.contains(marker))
}

fn is_temporal_type(data_type: &str) -> bool {
    let upper = data_type.to_uppercase();
    upper.contains("DATE") || upper.contains("TIMESTAMP")
}

fn readable(name: &str) -> String {
    name.replace('_', " ")
}

/// Table and column names in the readable form people type, for prompt autocomplete.
pub fn vocabulary(workspace: &Workspace) -> AppResult<Vec<String>> {
    let (candidates, _) = load(workspace)?;
    let mut terms = BTreeSet::new();
    for candidate in &candidates {
        terms.insert(readable(&candidate.table.relation));
        terms.extend(candidate.columns.iter().map(|column| readable(&column.name)));
    }
    Ok(terms.into_iter().collect())
}

/// Starter questions from the schema alone, so they appear instantly and never need a model.
pub fn starter_questions(workspace: &Workspace) -> AppResult<Vec<String>> {
    let (candidates, _) = load(workspace)?;
    let mut questions = Vec::new();
    for candidate in &candidates {
        let measure = candidate
            .columns
            .iter()
            .find(|column| is_numeric_type(&column.data_type) && !column.name.to_lowercase().ends_with("id"));
        let date = candidate.columns.iter().find(|column| is_temporal_type(&column.data_type));
        let dimension = candidate.columns.iter().find(|column| {
            column.data_type.to_uppercase().contains("VARCHAR")
                && column.profile.as_ref().is_some_and(|profile| (2..=50).contains(&profile.distinct_count))
        });
        let table = readable(&candidate.table.relation);
        match (measure, date, dimension) {
            (Some(measure), Some(_), Some(dimension)) => {
                questions.push(format!("Why did {} change last month?", readable(&measure.name)));
                questions.push(format!("Which {} has the highest total {}?", readable(&dimension.name), readable(&measure.name)));
            }
            (Some(measure), Some(_), None) => questions.push(format!("How has total {} changed by month?", readable(&measure.name))),
            (Some(measure), None, Some(dimension)) => {
                questions.push(format!("What is the total {} by {}?", readable(&measure.name), readable(&dimension.name)));
            }
            _ => questions.push(format!("How many {table} are there?")),
        }
    }
    if let Ok(db) = workspace.db() {
        if let Ok(Some(extra)) = db.workspace_setting::<Vec<String>>("template.questions") {
            for question in extra.into_iter().rev() {
                if !questions.contains(&question) {
                    questions.insert(0, question);
                }
            }
        }
    }
    questions.dedup();
    questions.truncate(STARTER_QUESTIONS);
    Ok(questions)
}

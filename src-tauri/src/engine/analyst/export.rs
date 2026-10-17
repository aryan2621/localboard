//! Markdown export of a conversation: questions, answers, assumptions, SQL and a result preview.

use std::fmt::Write;

use serde_json::Value;

use super::Answer;
use crate::engine::query::QueryResult;
use crate::storage::conversations::{MessageRecord, MessageRole, QueryRecord, QueryStatus};

const PREVIEW_ROWS: usize = 20;

fn cell(value: &Value) -> String {
    let text = match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    text.replace('|', "\\|").replace('\n', " ")
}

fn preview(out: &mut String, result: &QueryResult) {
    if result.columns.is_empty() {
        return;
    }
    let header: Vec<String> = result.columns.iter().map(|column| column.name.replace('|', "\\|")).collect();
    let _ = writeln!(out, "| {} |", header.join(" | "));
    let _ = writeln!(out, "|{}|", vec![" --- "; header.len()].join("|"));
    for row in result.rows.iter().take(PREVIEW_ROWS) {
        let cells: Vec<String> = row.iter().map(cell).collect();
        let _ = writeln!(out, "| {} |", cells.join(" | "));
    }
    if result.rows.len() > PREVIEW_ROWS || result.truncated {
        let _ = writeln!(out, "\n_Showing the first {} rows._", result.rows.len().min(PREVIEW_ROWS));
    }
    out.push('\n');
}

pub fn markdown(title: &str, messages: &[MessageRecord], queries: &[QueryRecord]) -> String {
    let mut out = format!("# {title}\n\n_Exported from LocalBoard._\n\n");
    for message in messages {
        match message.role {
            MessageRole::User => {
                let _ = writeln!(out, "## {}\n", message.content.trim());
            }
            MessageRole::Assistant => {
                let _ = writeln!(out, "{}\n", message.content.trim());
                let answer: Option<Answer> = message.answer.clone().and_then(|value| serde_json::from_value(value).ok());
                let Some(answer) = answer else { continue };
                for assumption in &answer.assumptions {
                    let _ = writeln!(out, "- Assumption: {assumption}");
                }
                for warning in &answer.warnings {
                    let _ = writeln!(out, "- Warning: {warning}");
                }
                if !answer.assumptions.is_empty() || !answer.warnings.is_empty() {
                    out.push('\n');
                }
                let successful = queries.iter().filter(|query| query.message_id == message.id && query.status == QueryStatus::Ok);
                for query in successful {
                    let _ = writeln!(out, "```sql\n{}\n```\n", query.sql);
                    if Some(&query.id) == answer.primary_query_id.as_ref() {
                        if let Some(result) = &query.result {
                            preview(&mut out, result);
                        }
                    }
                }
            }
        }
    }
    out
}

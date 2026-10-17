//! List and export the append-only audit log. Writes stay on the existing record_audit path.

use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use crate::error::{blocking, AppError, AppResult};
use crate::state::AppState;
use crate::storage::workspace_db::{AuditFilter, AuditPage, AuditRow};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExport {
    pub count: usize,
}

fn filter_of(kind: Option<String>, status: Option<String>, search: Option<String>, before_ts: Option<String>, before_id: Option<i64>) -> AuditFilter {
    AuditFilter {
        kind: nonempty(kind),
        status: nonempty(status),
        search: nonempty(search),
        before_ts: nonempty(before_ts),
        before_id,
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
}

#[tauri::command]
pub async fn audit_list(
    state: State<'_, AppState>,
    kind: Option<String>,
    status: Option<String>,
    search: Option<String>,
    before_ts: Option<String>,
    before_id: Option<i64>,
) -> AppResult<AuditPage> {
    let workspace = state.workspace()?;
    let filter = filter_of(kind, status, search, before_ts, before_id);
    blocking(move || workspace.db()?.list_audit(&filter)).await
}

#[tauri::command]
pub async fn audit_export(
    state: State<'_, AppState>,
    kind: Option<String>,
    status: Option<String>,
    search: Option<String>,
    format: String,
    path: String,
) -> AppResult<AuditExport> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let filter = filter_of(kind, status, search, None, None);
    let kind = format.to_ascii_lowercase();
    blocking(move || {
        let rows = workspace.db()?.export_audit(&filter)?;
        let target = PathBuf::from(&path);
        match kind.as_str() {
            "json" => std::fs::write(&target, serde_json::to_vec_pretty(&rows)?)?,
            "csv" => write_csv(&target, &rows)?,
            _ => return Err(AppError::invalid("Export as json or csv.")),
        }
        Ok(AuditExport { count: rows.len() })
    })
    .await
}

fn write_csv(path: &PathBuf, rows: &[AuditRow]) -> AppResult<()> {
    let mut out = String::from("id,ts,actor,kind,status,duration_ms,rows,question,sql,relations,detail_json\n");
    for row in rows {
        let detail = if row.detail.is_null() { String::new() } else { row.detail.to_string() };
        let cells = [
            row.id.to_string(),
            row.ts.clone(),
            row.actor.clone(),
            row.kind.clone(),
            row.status.clone(),
            row.duration_ms.map(|value| value.to_string()).unwrap_or_default(),
            row.rows.map(|value| value.to_string()).unwrap_or_default(),
            row.question.clone().unwrap_or_default(),
            row.sql.clone().unwrap_or_default(),
            row.relations.join("; "),
            detail,
        ];
        out.push_str(&cells.iter().map(|cell| csv_cell(cell)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn csv_cell(value: &str) -> String {
    // Spreadsheet apps run cells that start with these as formulas; a leading quote makes them text.
    let guarded;
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        guarded = format!("'{value}");
        guarded.as_str()
    } else {
        value
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

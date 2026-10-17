//! Workflow automations: triggers, ordered steps and per-step run history.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::security::{hex_of, tokens_match};
use super::workspace_db::WorkspaceDb;
use crate::error::{AppError, AppResult};

fn now() -> &'static str {
    "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationStep {
    pub id: String,
    pub sort: i64,
    pub kind: String,
    pub config: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRecord {
    pub id: String,
    pub name: String,
    pub trigger_kind: String,
    pub trigger: Value,
    pub steps: Vec<AutomationStep>,
    pub enabled: bool,
    pub on_change_only: bool,
    pub needs_review: bool,
    pub source: String,
    pub confirmed_at: Option<String>,
    pub last_run_at: Option<String>,
    pub last_status: Option<String>,
    pub last_summary: Option<String>,
    pub last_hash: Option<String>,
    pub has_webhook: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_token: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationStepRun {
    pub id: String,
    pub step_id: String,
    pub sort: i64,
    pub kind: String,
    pub status: String,
    pub summary: Option<String>,
    pub facts: Option<Value>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    pub id: String,
    pub trigger_kind: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub notified: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub steps: Vec<AutomationStepRun>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmtpSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub from: String,
    pub has_password: bool,
}

impl Default for SmtpSettings {
    fn default() -> Self {
        Self { enabled: false, host: String::new(), port: 587, username: String::new(), from: String::new(), has_password: false }
    }
}

pub fn hash_token(token: &str) -> String {
    hex_of(&Sha256::digest(token.as_bytes()))
}

pub fn new_webhook_token() -> String {
    hex_of(uuid::Uuid::new_v4().as_bytes()) + &hex_of(uuid::Uuid::new_v4().as_bytes())
}

impl WorkspaceDb {
    pub fn list_automations(&self) -> AppResult<Vec<AutomationRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, trigger_kind, trigger_json, enabled, on_change_only, needs_review, source, confirmed_at,
                    last_run_at, last_status, last_summary, last_hash, webhook_hash
             FROM automations ORDER BY name",
        )?;
        let rows = statement.query_map([], map_automation)?;
        rows.map(|row| row.map_err(AppError::from).and_then(|record| self.with_steps(record)))
            .collect()
    }

    pub fn enabled_automations(&self) -> AppResult<Vec<AutomationRecord>> {
        Ok(self.list_automations()?.into_iter().filter(|item| item.enabled && !item.needs_review).collect())
    }

    pub fn automation(&self, id: &str) -> AppResult<AutomationRecord> {
        let record = self
            .conn
            .query_row(
                "SELECT id, name, trigger_kind, trigger_json, enabled, on_change_only, needs_review, source, confirmed_at,
                        last_run_at, last_status, last_summary, last_hash, webhook_hash
                 FROM automations WHERE id = ?1",
                [id],
                map_automation,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Automation not found"))?;
        self.with_steps(record)
    }

    pub fn upsert_automation(&self, record: &AutomationRecord, webhook_hash: Option<&str>) -> AppResult<AutomationRecord> {
        self.conn.execute(
            &format!(
                "INSERT INTO automations(
                   id, name, trigger_kind, trigger_json, enabled, on_change_only, needs_review, source, confirmed_at, webhook_hash
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   trigger_kind = excluded.trigger_kind,
                   trigger_json = excluded.trigger_json,
                   enabled = excluded.enabled,
                   on_change_only = excluded.on_change_only,
                   needs_review = excluded.needs_review,
                   source = excluded.source,
                   confirmed_at = excluded.confirmed_at,
                   webhook_hash = COALESCE(excluded.webhook_hash, automations.webhook_hash),
                   updated_at = {now}",
                now = now()
            ),
            params![
                record.id,
                record.name,
                record.trigger_kind,
                serde_json::to_string(&record.trigger)?,
                record.enabled as i64,
                record.on_change_only as i64,
                record.needs_review as i64,
                record.source,
                record.confirmed_at,
                webhook_hash
            ],
        )?;
        self.replace_steps(&record.id, &record.steps)?;
        self.automation(&record.id)
    }

    pub fn confirm_automation(&self, id: &str) -> AppResult<AutomationRecord> {
        let changed = self.conn.execute(
            &format!("UPDATE automations SET needs_review = 0, confirmed_at = {now}, updated_at = {now} WHERE id = ?1", now = now()),
            [id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Automation not found"));
        }
        self.automation(id)
    }

    pub fn set_webhook_hash(&self, id: &str, hash: &str) -> AppResult<()> {
        let changed = self.conn.execute("UPDATE automations SET webhook_hash = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2", params![hash, id])?;
        if changed == 0 {
            return Err(AppError::not_found("Automation not found"));
        }
        Ok(())
    }

    pub fn webhook_hash(&self, id: &str) -> AppResult<Option<String>> {
        self.conn
            .query_row("SELECT webhook_hash FROM automations WHERE id = ?1", [id], |row| row.get(0))
            .optional()?
            .ok_or_else(|| AppError::not_found("Automation not found"))
    }

    pub fn webhook_matches(&self, id: &str, token: &str) -> AppResult<bool> {
        match self.webhook_hash(id)? {
            Some(stored) => Ok(tokens_match(&stored, &hash_token(token))),
            None => Ok(false),
        }
    }

    pub fn delete_automation(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM automations WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Automation not found"));
        }
        Ok(())
    }

    pub fn start_automation_run(&self, id: &str, automation_id: &str, trigger_kind: &str) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO automation_runs(id, automation_id, trigger_kind, status) VALUES (?1, ?2, ?3, 'running')",
            params![id, automation_id, trigger_kind],
        )?;
        Ok(())
    }

    pub fn start_step_run(&self, id: &str, run_id: &str, step_id: &str, sort: i64, kind: &str) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO automation_step_runs(id, run_id, step_id, sort, kind, status) VALUES (?1, ?2, ?3, ?4, ?5, 'running')",
            params![id, run_id, step_id, sort, kind],
        )?;
        Ok(())
    }

    pub fn finish_step_run(&self, id: &str, status: &str, summary: &str, facts: &Value) -> AppResult<()> {
        self.conn.execute(
            &format!("UPDATE automation_step_runs SET status = ?1, summary = ?2, facts_json = ?3, finished_at = {now} WHERE id = ?4", now = now()),
            params![status, summary, serde_json::to_string(facts)?, id],
        )?;
        Ok(())
    }

    pub fn finish_automation_run(
        &self,
        automation_id: &str,
        run_id: &str,
        status: &str,
        summary: &str,
        hash: &str,
        notified: bool,
    ) -> AppResult<()> {
        self.conn.execute(
            &format!(
                "UPDATE automations SET last_run_at = {now}, last_status = ?1, last_summary = ?2, last_hash = ?3, updated_at = {now} WHERE id = ?4",
                now = now()
            ),
            params![status, summary, hash, automation_id],
        )?;
        self.conn.execute(
            &format!("UPDATE automation_runs SET status = ?1, summary = ?2, notified = ?3, finished_at = {now} WHERE id = ?4", now = now()),
            params![status, summary, notified as i64, run_id],
        )?;
        Ok(())
    }

    pub fn last_ok_facts(&self, automation_id: &str) -> AppResult<Option<Value>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT sr.facts_json FROM automation_step_runs sr
                 JOIN automation_runs r ON r.id = sr.run_id
                 WHERE r.automation_id = ?1 AND r.status = 'ok' AND sr.status = 'ok' AND sr.facts_json IS NOT NULL
                 ORDER BY r.started_at DESC, sr.sort DESC LIMIT 1",
                [automation_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|text| serde_json::from_str(&text)).transpose().map_err(AppError::from)
    }

    pub fn automation_runs(&self, automation_id: &str) -> AppResult<Vec<AutomationRun>> {
        let mut statement = self.conn.prepare(
            "SELECT id, trigger_kind, status, summary, notified, started_at, finished_at FROM automation_runs
             WHERE automation_id = ?1 ORDER BY started_at DESC LIMIT 30",
        )?;
        let rows = statement.query_map([automation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)? == 1,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut runs = Vec::new();
        for row in rows {
            let (id, trigger_kind, status, summary, notified, started_at, finished_at) = row?;
            let steps = self.step_runs(&id)?;
            runs.push(AutomationRun { id, trigger_kind, status, summary, notified, started_at, finished_at, steps });
        }
        Ok(runs)
    }

    pub fn workspace_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> AppResult<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value_json FROM workspace_settings WHERE key = ?1", [key], |row| row.get(0))
            .optional()?;
        raw.map(|json| serde_json::from_str(&json)).transpose().map_err(AppError::from)
    }

    pub fn set_workspace_setting<T: Serialize>(&self, key: &str, value: &T) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO workspace_settings(key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    fn with_steps(&self, mut record: AutomationRecord) -> AppResult<AutomationRecord> {
        record.steps = self.steps(&record.id)?;
        Ok(record)
    }

    fn steps(&self, automation_id: &str) -> AppResult<Vec<AutomationStep>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, sort, kind, config_json FROM automation_steps WHERE automation_id = ?1 ORDER BY sort")?;
        let rows = statement.query_map([automation_id], |row| {
            let config: String = row.get(3)?;
            Ok(AutomationStep {
                id: row.get(0)?,
                sort: row.get(1)?,
                kind: row.get(2)?,
                config: serde_json::from_str(&config).unwrap_or(Value::Null),
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    fn replace_steps(&self, automation_id: &str, steps: &[AutomationStep]) -> AppResult<()> {
        self.conn.execute("DELETE FROM automation_steps WHERE automation_id = ?1", [automation_id])?;
        for step in steps {
            self.conn.execute(
                "INSERT INTO automation_steps(id, automation_id, sort, kind, config_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![step.id, automation_id, step.sort, step.kind, serde_json::to_string(&step.config)?],
            )?;
        }
        Ok(())
    }

    fn step_runs(&self, run_id: &str) -> AppResult<Vec<AutomationStepRun>> {
        let mut statement = self.conn.prepare(
            "SELECT id, step_id, sort, kind, status, summary, facts_json, started_at, finished_at
             FROM automation_step_runs WHERE run_id = ?1 ORDER BY sort",
        )?;
        let rows = statement.query_map([run_id], |row| {
            let facts: Option<String> = row.get(6)?;
            Ok(AutomationStepRun {
                id: row.get(0)?,
                step_id: row.get(1)?,
                sort: row.get(2)?,
                kind: row.get(3)?,
                status: row.get(4)?,
                summary: row.get(5)?,
                facts: facts.and_then(|text| serde_json::from_str(&text).ok()),
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }
}

fn map_automation(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRecord> {
    let trigger: String = row.get(3)?;
    let webhook: Option<String> = row.get(13)?;
    Ok(AutomationRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        trigger_kind: row.get(2)?,
        trigger: serde_json::from_str(&trigger).unwrap_or(Value::Null),
        steps: Vec::new(),
        enabled: row.get::<_, i64>(4)? == 1,
        on_change_only: row.get::<_, i64>(5)? == 1,
        needs_review: row.get::<_, i64>(6)? == 1,
        source: row.get(7)?,
        confirmed_at: row.get(8)?,
        last_run_at: row.get(9)?,
        last_status: row.get(10)?,
        last_summary: row.get(11)?,
        last_hash: row.get(12)?,
        has_webhook: webhook.is_some(),
        webhook_url: None,
        webhook_token: None,
    })
}

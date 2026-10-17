//! Datasites, PDF reports and presentation decks.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::workspace_db::WorkspaceDb;
use crate::engine::analyst::chart::ChartSpec;
use crate::engine::query::QueryResult;
use crate::error::{AppError, AppResult};

fn now() -> &'static str {
    "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub id: String,
    pub name: String,
    pub updated_at: String,
    pub item_count: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasiteChart {
    pub id: String,
    pub page_id: String,
    pub title: String,
    pub sql: String,
    pub chart: ChartSpec,
    pub result: Option<QueryResult>,
    pub facts: Option<Value>,
    pub sort: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasitePage {
    pub id: String,
    pub datasite_id: String,
    pub slug: String,
    pub title: String,
    pub sort: i64,
    pub narrative: String,
    pub charts: Vec<DatasiteChart>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasiteRecord {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub source_message_id: Option<String>,
    pub pages: Vec<DatasitePage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportBlock {
    pub id: String,
    pub report_id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub sql: Option<String>,
    pub chart: Option<ChartSpec>,
    pub result: Option<QueryResult>,
    pub facts: Option<Value>,
    pub sort: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRecord {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub theme: Value,
    pub source_message_id: Option<String>,
    pub blocks: Vec<ReportBlock>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineSlide {
    pub id: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckSlide {
    pub id: String,
    pub deck_id: String,
    pub title: String,
    pub body: String,
    pub sql: Option<String>,
    pub chart: Option<ChartSpec>,
    pub result: Option<QueryResult>,
    pub sort: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckRecord {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub outline: Vec<OutlineSlide>,
    pub source_message_id: Option<String>,
    pub slides: Vec<DeckSlide>,
}

impl WorkspaceDb {
    pub fn list_datasites(&self) -> AppResult<Vec<ArtifactSummary>> {
        list_summaries(&self.conn, "datasites", "SELECT COUNT(*) FROM datasite_pages p WHERE p.datasite_id = d.id")
    }

    pub fn datasite(&self, id: &str) -> AppResult<DatasiteRecord> {
        let (name, prompt, source): (String, String, Option<String>) = self
            .conn
            .query_row("SELECT name, prompt, source_message_id FROM datasites WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?
            .ok_or_else(|| AppError::not_found("Datasite not found"))?;
        Ok(DatasiteRecord { id: id.to_owned(), name, prompt, source_message_id: source, pages: self.datasite_pages(id)? })
    }

    pub fn insert_datasite(&self, record: &DatasiteRecord) -> AppResult<DatasiteRecord> {
        self.conn.execute(
            "INSERT INTO datasites(id, name, prompt, source_message_id) VALUES (?1, ?2, ?3, ?4)",
            params![record.id, record.name, record.prompt, record.source_message_id],
        )?;
        for page in &record.pages {
            self.conn.execute(
                "INSERT INTO datasite_pages(id, datasite_id, slug, title, sort, narrative) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![page.id, page.datasite_id, page.slug, page.title, page.sort, page.narrative],
            )?;
            for chart in &page.charts {
                self.insert_datasite_chart(chart)?;
            }
        }
        self.datasite(&record.id)
    }

    fn insert_datasite_chart(&self, chart: &DatasiteChart) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO datasite_charts(id, page_id, title, sql, chart_json, result_json, facts_json, sort)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                chart.id,
                chart.page_id,
                chart.title,
                chart.sql,
                serde_json::to_string(&chart.chart)?,
                opt_json(&chart.result)?,
                opt_json(&chart.facts)?,
                chart.sort
            ],
        )?;
        Ok(())
    }

    pub fn set_datasite_chart_snapshot(&self, id: &str, result: &QueryResult, facts: &Value) -> AppResult<()> {
        self.conn.execute(
            "UPDATE datasite_charts SET result_json = ?1, facts_json = ?2 WHERE id = ?3",
            params![serde_json::to_string(result)?, serde_json::to_string(facts)?, id],
        )?;
        Ok(())
    }

    pub fn delete_datasite(&self, id: &str) -> AppResult<()> {
        let charts = self.datasite_chart_ids(id)?;
        for chart in &charts {
            self.clear_query_relations("datasite", chart)?;
        }
        if self.conn.execute("DELETE FROM datasites WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Datasite not found"));
        }
        Ok(())
    }

    fn datasite_chart_ids(&self, datasite_id: &str) -> AppResult<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT c.id FROM datasite_charts c JOIN datasite_pages p ON p.id = c.page_id WHERE p.datasite_id = ?1",
        )?;
        let rows = statement.query_map([datasite_id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    fn datasite_pages(&self, datasite_id: &str) -> AppResult<Vec<DatasitePage>> {
        let mut statement = self.conn.prepare(
            "SELECT id, datasite_id, slug, title, sort, narrative FROM datasite_pages WHERE datasite_id = ?1 ORDER BY sort, title",
        )?;
        let rows = statement.query_map([datasite_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut pages = Vec::new();
        for row in rows {
            let (id, datasite_id, slug, title, sort, narrative) = row?;
            let charts = self.datasite_charts(&id)?;
            pages.push(DatasitePage { id, datasite_id, slug, title, sort, narrative, charts });
        }
        Ok(pages)
    }

    fn datasite_charts(&self, page_id: &str) -> AppResult<Vec<DatasiteChart>> {
        let mut statement = self.conn.prepare(
            "SELECT id, page_id, title, sql, chart_json, result_json, facts_json, sort FROM datasite_charts WHERE page_id = ?1 ORDER BY sort",
        )?;
        let rows = statement.query_map([page_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (id, page_id, title, sql, chart, result, facts, sort) = row?;
            Ok(DatasiteChart {
                id,
                page_id,
                title,
                sql,
                chart: serde_json::from_str(&chart)?,
                result: parse_opt(&result)?,
                facts: parse_opt(&facts)?,
                sort,
            })
        })
        .collect()
    }

    pub fn list_reports(&self) -> AppResult<Vec<ArtifactSummary>> {
        list_summaries(&self.conn, "reports", "SELECT COUNT(*) FROM report_blocks b WHERE b.report_id = d.id")
    }

    pub fn report(&self, id: &str) -> AppResult<ReportRecord> {
        let (name, prompt, theme, source): (String, String, String, Option<String>) = self
            .conn
            .query_row("SELECT name, prompt, theme_json, source_message_id FROM reports WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .optional()?
            .ok_or_else(|| AppError::not_found("Report not found"))?;
        Ok(ReportRecord { id: id.to_owned(), name, prompt, theme: serde_json::from_str(&theme)?, source_message_id: source, blocks: self.report_blocks(id)? })
    }

    pub fn insert_report(&self, record: &ReportRecord) -> AppResult<ReportRecord> {
        self.conn.execute(
            "INSERT INTO reports(id, name, prompt, theme_json, source_message_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![record.id, record.name, record.prompt, serde_json::to_string(&record.theme)?, record.source_message_id],
        )?;
        for block in &record.blocks {
            self.conn.execute(
                "INSERT INTO report_blocks(id, report_id, kind, title, body, sql, chart_json, result_json, facts_json, sort)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    block.id,
                    block.report_id,
                    block.kind,
                    block.title,
                    block.body,
                    block.sql,
                    opt_json(&block.chart)?,
                    opt_json(&block.result)?,
                    opt_json(&block.facts)?,
                    block.sort
                ],
            )?;
        }
        self.report(&record.id)
    }

    pub fn set_report_block_snapshot(&self, id: &str, result: &QueryResult, facts: &Value) -> AppResult<()> {
        self.conn.execute(
            "UPDATE report_blocks SET result_json = ?1, facts_json = ?2 WHERE id = ?3",
            params![serde_json::to_string(result)?, serde_json::to_string(facts)?, id],
        )?;
        Ok(())
    }

    pub fn delete_report(&self, id: &str) -> AppResult<()> {
        let blocks = self.report_sql_ids(id)?;
        for block in &blocks {
            self.clear_query_relations("report", block)?;
        }
        if self.conn.execute("DELETE FROM reports WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Report not found"));
        }
        Ok(())
    }

    fn report_sql_ids(&self, report_id: &str) -> AppResult<Vec<String>> {
        let mut statement = self.conn.prepare("SELECT id FROM report_blocks WHERE report_id = ?1 AND sql IS NOT NULL")?;
        let rows = statement.query_map([report_id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    fn report_blocks(&self, report_id: &str) -> AppResult<Vec<ReportBlock>> {
        let mut statement = self.conn.prepare(
            "SELECT id, report_id, kind, title, body, sql, chart_json, result_json, facts_json, sort
             FROM report_blocks WHERE report_id = ?1 ORDER BY sort",
        )?;
        let rows = statement.query_map([report_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })?;
        rows.map(|row| {
            let (id, report_id, kind, title, body, sql, chart, result, facts, sort) = row?;
            Ok(ReportBlock {
                id,
                report_id,
                kind,
                title,
                body,
                sql,
                chart: parse_opt(&chart)?,
                result: parse_opt(&result)?,
                facts: parse_opt(&facts)?,
                sort,
            })
        })
        .collect()
    }

    pub fn list_decks(&self) -> AppResult<Vec<ArtifactSummary>> {
        list_summaries(&self.conn, "decks", "SELECT COUNT(*) FROM deck_slides s WHERE s.deck_id = d.id")
    }

    pub fn deck(&self, id: &str) -> AppResult<DeckRecord> {
        let (name, prompt, outline, source): (String, String, String, Option<String>) = self
            .conn
            .query_row("SELECT name, prompt, outline_json, source_message_id FROM decks WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .optional()?
            .ok_or_else(|| AppError::not_found("Presentation not found"))?;
        Ok(DeckRecord { id: id.to_owned(), name, prompt, outline: serde_json::from_str(&outline)?, source_message_id: source, slides: self.deck_slides(id)? })
    }

    pub fn insert_deck(&self, record: &DeckRecord) -> AppResult<DeckRecord> {
        self.conn.execute(
            "INSERT INTO decks(id, name, prompt, outline_json, source_message_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![record.id, record.name, record.prompt, serde_json::to_string(&record.outline)?, record.source_message_id],
        )?;
        for slide in &record.slides {
            self.conn.execute(
                "INSERT INTO deck_slides(id, deck_id, title, body, sql, chart_json, result_json, sort)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    slide.id,
                    slide.deck_id,
                    slide.title,
                    slide.body,
                    slide.sql,
                    opt_json(&slide.chart)?,
                    opt_json(&slide.result)?,
                    slide.sort
                ],
            )?;
        }
        self.deck(&record.id)
    }

    /// Saves the outline as the deck's slide list: edits titles and bodies, follows the new order,
    /// drops slides that were removed and adds new text-only slides.
    pub fn update_deck_outline(&self, id: &str, outline: &[OutlineSlide]) -> AppResult<DeckRecord> {
        if outline.is_empty() {
            return Err(AppError::invalid("A presentation needs at least one slide"));
        }
        if outline.iter().any(|slide| slide.title.trim().is_empty()) {
            return Err(AppError::invalid("Every slide needs a title"));
        }
        let tx = self.conn.unchecked_transaction()?;
        let existing: Vec<String> = {
            let mut statement = tx.prepare("SELECT id FROM deck_slides WHERE deck_id = ?1")?;
            let rows = statement.query_map([id], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for slide_id in existing.iter().filter(|slide_id| !outline.iter().any(|slide| &slide.id == *slide_id)) {
            self.clear_query_relations("deck", slide_id)?;
            tx.execute("DELETE FROM deck_slides WHERE id = ?1 AND deck_id = ?2", params![slide_id, id])?;
        }
        for (index, slide) in outline.iter().enumerate() {
            let (title, body, sort) = (slide.title.trim(), slide.body.trim(), index as i64);
            if existing.contains(&slide.id) {
                tx.execute(
                    "UPDATE deck_slides SET title = ?1, body = ?2, sort = ?3 WHERE id = ?4 AND deck_id = ?5",
                    params![title, body, sort, slide.id, id],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO deck_slides(id, deck_id, title, body, sql, chart_json, result_json, sort)
                     VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, ?5)",
                    params![slide.id, id, title, body, sort],
                )?;
            }
        }
        let changed = tx.execute(
            &format!("UPDATE decks SET outline_json = ?1, updated_at = {now} WHERE id = ?2", now = now()),
            params![serde_json::to_string(outline)?, id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Presentation not found"));
        }
        tx.commit()?;
        self.deck(id)
    }

    pub fn set_deck_slide_snapshot(&self, id: &str, result: &QueryResult) -> AppResult<()> {
        self.conn.execute("UPDATE deck_slides SET result_json = ?1 WHERE id = ?2", params![serde_json::to_string(result)?, id])?;
        Ok(())
    }

    pub fn delete_deck(&self, id: &str) -> AppResult<()> {
        let slides = self.deck_sql_ids(id)?;
        for slide in &slides {
            self.clear_query_relations("deck", slide)?;
        }
        if self.conn.execute("DELETE FROM decks WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Presentation not found"));
        }
        Ok(())
    }

    fn deck_sql_ids(&self, deck_id: &str) -> AppResult<Vec<String>> {
        let mut statement = self.conn.prepare("SELECT id FROM deck_slides WHERE deck_id = ?1 AND sql IS NOT NULL")?;
        let rows = statement.query_map([deck_id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    fn deck_slides(&self, deck_id: &str) -> AppResult<Vec<DeckSlide>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, deck_id, title, body, sql, chart_json, result_json, sort FROM deck_slides WHERE deck_id = ?1 ORDER BY sort")?;
        let rows = statement.query_map([deck_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (id, deck_id, title, body, sql, chart, result, sort) = row?;
            Ok(DeckSlide {
                id,
                deck_id,
                title,
                body,
                sql,
                chart: parse_opt(&chart)?,
                result: parse_opt(&result)?,
                sort,
            })
        })
        .collect()
    }
}

fn list_summaries(conn: &rusqlite::Connection, table: &str, count_sql: &str) -> AppResult<Vec<ArtifactSummary>> {
    let sql = format!("SELECT d.id, d.name, d.updated_at, ({count_sql}) FROM {table} d ORDER BY d.updated_at DESC");
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        Ok(ArtifactSummary { id: row.get(0)?, name: row.get(1)?, updated_at: row.get(2)?, item_count: row.get(3)? })
    })?;
    rows.collect::<Result<_, _>>().map_err(AppError::from)
}

fn opt_json<T: Serialize>(value: &Option<T>) -> AppResult<Option<String>> {
    match value {
        Some(value) => Ok(Some(serde_json::to_string(value)?)),
        None => Ok(None),
    }
}

fn parse_opt<T: for<'de> Deserialize<'de>>(raw: &Option<String>) -> AppResult<Option<T>> {
    match raw {
        Some(raw) if !raw.is_empty() => Ok(Some(serde_json::from_str(raw)?)),
        _ => Ok(None),
    }
}

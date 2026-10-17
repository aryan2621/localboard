//! Prompts, HTML, PDF and PPTX for datasites, reports and decks.

pub mod html;
pub mod pdf;
pub mod pptx;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::engine::analyst::chart::ChartSpec;
use crate::engine::analyst::facts;
use crate::engine::analyst::Answer;
use crate::engine::query::QueryResult;
use crate::storage::conversations::{QueryRecord, QueryStatus};
use crate::storage::conversations::snapshot;

pub const ARTIFACT_MAX_TOKENS: u32 = 2_048;
pub const BRAND_HEX: &str = "#C96240";
pub const BRAND_NAME: &str = "LocalBoard";

pub fn theme_of(theme: &crate::storage::theme::WorkspaceTheme) -> Value {
    json!({ "brand": theme.brand, "name": theme.name })
}

pub fn default_theme() -> Value {
    json!({ "brand": BRAND_HEX, "name": BRAND_NAME })
}

pub fn datasite_system(schema: &str) -> String {
    format!(
        "Propose a multi-page datasite. Return a short name, an overview page, and up to 5 sections.\n\
         Each page has a title, a narrative with no numbers, and up to 4 tiles.\n\
         Each tile is a title, one read-only SELECT, and a chart kind.\n\
         Do not invent tables or columns. Do not put numbers in the narrative.\n\nSCHEMA:\n{schema}"
    )
}

fn page_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "narrative": { "type": "string" },
            "charts": {
                "type": "array",
                "maxItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "sql": { "type": "string" },
                        "chart": { "type": "string", "enum": ["kpi", "line", "bar", "stacked", "area", "pie", "table", "funnel", "heatmap"] }
                    },
                    "required": ["title", "sql", "chart"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["title", "narrative", "charts"],
        "additionalProperties": false
    })
}

pub fn datasite_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "overview": page_schema(),
            "sections": { "type": "array", "maxItems": 5, "items": page_schema() }
        },
        "required": ["name", "overview", "sections"],
        "additionalProperties": false
    })
}

pub fn report_system(schema: &str) -> String {
    format!(
        "Propose a written report. Return a short name and up to 12 blocks.\n\
         A block is narrative (title and body, no numbers), chart, or table.\n\
         Chart and table blocks have a title, one read-only SELECT, and a chart kind.\n\
         Do not invent tables or columns.\n\nSCHEMA:\n{schema}"
    )
}

pub fn report_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "blocks": {
                "type": "array",
                "maxItems": 12,
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["narrative", "chart", "table"] },
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "sql": { "type": "string" },
                        "chart": { "type": "string", "enum": ["kpi", "line", "bar", "stacked", "area", "pie", "table", "funnel", "heatmap"] }
                    },
                    "required": ["kind", "title", "body", "sql", "chart"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["name", "blocks"],
        "additionalProperties": false
    })
}

pub fn deck_system(schema: &str) -> String {
    format!(
        "Propose a slide outline from the request. Return a short name and up to 12 slides.\n\
         Each slide has a title, bullet body with no numbers, and optional SQL plus a chart kind.\n\
         Leave sql empty when the slide is title-only or conclusions.\n\
         Do not invent tables or columns.\n\nSCHEMA:\n{schema}"
    )
}

pub fn deck_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "slides": {
                "type": "array",
                "maxItems": 12,
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "sql": { "type": "string" },
                        "chart": { "type": "string", "enum": ["kpi", "line", "bar", "stacked", "area", "pie", "table", "funnel", "heatmap"] }
                    },
                    "required": ["title", "body", "sql", "chart"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["name", "slides"],
        "additionalProperties": false
    })
}

#[derive(Deserialize)]
pub struct ProposedPage {
    pub title: String,
    pub narrative: String,
    pub charts: Vec<ProposedChart>,
}

#[derive(Deserialize)]
pub struct ProposedDatasite {
    pub name: String,
    pub overview: ProposedPage,
    pub sections: Vec<ProposedPage>,
}

#[derive(Deserialize)]
pub struct ProposedChart {
    pub title: String,
    pub sql: String,
    pub chart: String,
}

#[derive(Deserialize)]
pub struct ProposedReport {
    pub name: String,
    pub blocks: Vec<ProposedBlock>,
}

#[derive(Deserialize)]
pub struct ProposedBlock {
    pub kind: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub sql: String,
    #[serde(default)]
    pub chart: String,
}

#[derive(Deserialize)]
pub struct ProposedDeck {
    pub name: String,
    pub slides: Vec<ProposedSlide>,
}

#[derive(Deserialize)]
pub struct ProposedSlide {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub sql: String,
    #[serde(default)]
    pub chart: String,
}

pub fn slug(title: &str, used: &mut std::collections::HashSet<String>) -> String {
    let mut base: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while base.contains("--") {
        base = base.replace("--", "-");
    }
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "page".to_owned() } else { base.to_owned() };
    let mut candidate = base.clone();
    let mut n = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

pub fn facts_of(result: &QueryResult) -> Value {
    serde_json::to_value(facts::compute(result)).unwrap_or(Value::Null)
}

pub fn snap(result: &QueryResult) -> QueryResult {
    snapshot(result)
}

/// Successful queries on a stored answer, with the chart the analyst already picked.
pub fn answer_charts(answer: &Answer, queries: &[QueryRecord]) -> Vec<(String, String, ChartSpec, QueryResult, Vec<String>)> {
    let mut items = Vec::new();
    for query in queries {
        if query.status != QueryStatus::Ok {
            continue;
        }
        let Some(result) = query.result.as_ref() else { continue };
        if query.sql.trim().is_empty() {
            continue;
        }
        let spec = spec_for(answer, query, result);
        items.push((query.id.clone(), title_for(answer, query), spec, result.clone(), query.relations.clone()));
    }
    items
}

fn spec_for(answer: &Answer, query: &QueryRecord, result: &QueryResult) -> ChartSpec {
    if let Some(chart) = &answer.chart {
        if chart.query_id == query.id {
            return chart.clone();
        }
    }
    for part in &answer.parts {
        if part.chart.query_id == query.id {
            return part.chart.clone();
        }
    }
    crate::engine::analyst::chart::pick(&query.id, result, crate::engine::analyst::chart::ChartHint::Auto)
}

fn title_for(answer: &Answer, query: &QueryRecord) -> String {
    for part in &answer.parts {
        if part.chart.query_id == query.id && !part.question.trim().is_empty() {
            return part.question.clone();
        }
    }
    if answer.chart.as_ref().is_some_and(|chart| chart.query_id == query.id) {
        return "Result".to_owned();
    }
    "Result".to_owned()
}

pub fn answer_text(answer: &Answer, content: &str) -> String {
    let text = content.trim();
    if !text.is_empty() {
        return text.to_owned();
    }
    answer.explanation.clone().unwrap_or_default()
}

pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn json_script(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into()).replace('<', "\\u003c")
}

pub fn display_name(name: &str, fallback: &str) -> String {
    let name = name.trim();
    if name.is_empty() { fallback.to_owned() } else { name.to_owned() }
}

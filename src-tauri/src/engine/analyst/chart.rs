use serde::{Deserialize, Serialize};

use crate::engine::query::{ColumnKind, QueryResult};

const MAX_BAR_ROWS: usize = 50;
const MAX_PIE_SLICES: usize = 6;
const MAX_SERIES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    Kpi,
    Line,
    Bar,
    Stacked,
    Area,
    Pie,
    Table,
    Funnel,
    Heatmap,
    /// Markdown note tile; the markdown is kept in the tile's `sql` column and never executed.
    Text,
}

/// The model's suggestion; only honoured when the result's shape supports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartHint {
    Auto,
    Kpi,
    Line,
    Bar,
    Stacked,
    Area,
    Pie,
    Table,
    Funnel,
    Heatmap,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub query_id: String,
    pub x: Option<String>,
    pub y: Vec<String>,
    /// Series drawn dashed because they are projections rather than data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashed: Vec<String>,
}

pub fn pick(query_id: &str, result: &QueryResult, hint: ChartHint) -> ChartSpec {
    let spec = |kind, x: Option<&str>, y: Vec<&str>| ChartSpec {
        kind,
        query_id: query_id.to_owned(),
        x: x.map(str::to_owned),
        y: y.into_iter().map(str::to_owned).collect(),
        dashed: Vec::new(),
    };
    let rows = result.rows.len();
    let first_of = |kind: ColumnKind| result.columns.iter().find(|column| column.kind == kind).map(|column| column.name.as_str());
    let numbers_except = |x: Option<&str>| {
        result
            .columns
            .iter()
            .filter(|column| column.kind == ColumnKind::Number && Some(column.name.as_str()) != x)
            .map(|column| column.name.as_str())
            .take(MAX_SERIES)
            .collect::<Vec<_>>()
    };

    if hint == ChartHint::Table || rows == 0 {
        return spec(ChartKind::Table, None, Vec::new());
    }
    if rows == 1 {
        let numbers = numbers_except(None);
        return if numbers.is_empty() { spec(ChartKind::Table, None, Vec::new()) } else { spec(ChartKind::Kpi, None, numbers) };
    }
    if hint == ChartHint::Heatmap {
        if let Some(heat) = heatmap(result) {
            return spec(ChartKind::Heatmap, Some(heat.0.as_str()), heat.1.iter().map(String::as_str).collect());
        }
    }
    if let Some(time) = first_of(ColumnKind::Temporal) {
        let numbers = numbers_except(Some(time));
        if !numbers.is_empty() {
            let kind = match hint {
                ChartHint::Bar => ChartKind::Bar,
                ChartHint::Stacked => ChartKind::Stacked,
                ChartHint::Area => ChartKind::Area,
                _ => ChartKind::Line,
            };
            return spec(kind, Some(time), numbers);
        }
    }
    if let Some(category) = first_of(ColumnKind::Text) {
        let numbers = numbers_except(Some(category));
        if !numbers.is_empty() && rows <= MAX_BAR_ROWS {
            if (hint == ChartHint::Pie || hint == ChartHint::Funnel) && rows <= MAX_PIE_SLICES && numbers.len() == 1 {
                return spec(if hint == ChartHint::Funnel { ChartKind::Funnel } else { ChartKind::Pie }, Some(category), numbers);
            }
            if hint == ChartHint::Line {
                return spec(ChartKind::Line, Some(category), numbers);
            }
            if hint == ChartHint::Area {
                return spec(ChartKind::Area, Some(category), numbers);
            }
            if hint == ChartHint::Stacked && numbers.len() > 1 {
                return spec(ChartKind::Stacked, Some(category), numbers);
            }
            return spec(ChartKind::Bar, Some(category), numbers);
        }
    }
    // A numeric first column (e.g. a year) can be an axis when the model asked for a line.
    if hint == ChartHint::Line {
        if let Some(axis) = result.columns.first().filter(|column| column.kind == ColumnKind::Number) {
            let numbers = numbers_except(Some(&axis.name));
            if !numbers.is_empty() {
                return spec(ChartKind::Line, Some(&axis.name), numbers);
            }
        }
    }
    spec(ChartKind::Table, None, Vec::new())
}

fn heatmap(result: &QueryResult) -> Option<(String, Vec<String>)> {
    let categories: Vec<&str> = result
        .columns
        .iter()
        .filter(|column| matches!(column.kind, ColumnKind::Text | ColumnKind::Temporal))
        .map(|column| column.name.as_str())
        .take(2)
        .collect();
    let measure = result.columns.iter().find(|column| column.kind == ColumnKind::Number)?.name.as_str();
    (categories.len() == 2).then(|| (categories[0].to_owned(), vec![categories[1].to_owned(), measure.to_owned()]))
}

pub fn hint_of(name: &str) -> ChartHint {
    match name {
        "kpi" => ChartHint::Kpi,
        "line" => ChartHint::Line,
        "bar" => ChartHint::Bar,
        "stacked" => ChartHint::Stacked,
        "area" => ChartHint::Area,
        "pie" => ChartHint::Pie,
        "funnel" => ChartHint::Funnel,
        "heatmap" => ChartHint::Heatmap,
        "table" => ChartHint::Table,
        _ => ChartHint::Auto,
    }
}

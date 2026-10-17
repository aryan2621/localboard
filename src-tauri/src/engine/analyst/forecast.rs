//! Forecasts are deterministic SQL wrapped around the model's historical series, so every
//! projected number has a query behind it: seasonal-naive with drift when at least two full
//! seasons of history exist, otherwise a linear trend. The model only narrates the result.

use std::collections::HashSet;

use serde_json::{json, Value};

use super::facts::{as_f64, round};
use super::{AnalystError, Run, StepStatus};
use crate::engine::duck::quote_ident;
use crate::engine::query::{ColumnKind, QueryResult};
use crate::storage::conversations::{QueryPurpose, QueryRecord};

pub const DEFAULT_PERIODS: u32 = 3;
pub const MAX_PERIODS: u32 = 24;
const MIN_HISTORY: usize = 3;

/// Column positions in the forecast query's output.
const PERIOD: usize = 0;
const FORECAST: usize = 2;
const METHOD: usize = 3;
pub const FORECAST_COLUMN: &str = "forecast";

struct Series {
    period: String,
    value: String,
}

/// The history must be one temporal column and one numeric column with one row per period.
fn series(result: &QueryResult) -> Result<Series, String> {
    let period = result
        .columns
        .iter()
        .position(|column| column.kind == ColumnKind::Temporal)
        .ok_or("A forecast needs a date column in the result.")?;
    let value = result
        .columns
        .iter()
        .enumerate()
        .position(|(index, column)| index != period && column.kind == ColumnKind::Number)
        .ok_or("A forecast needs a numeric column in the result.")?;
    let points: Vec<&Value> = result
        .rows
        .iter()
        .filter(|row| row.get(value).and_then(as_f64).is_some())
        .filter_map(|row| row.get(period).filter(|cell| !cell.is_null()))
        .collect();
    if points.len() < MIN_HISTORY {
        return Err(format!("A forecast needs at least {MIN_HISTORY} periods of history."));
    }
    let mut seen = HashSet::new();
    if !points.iter().all(|cell| seen.insert(cell.to_string())) {
        return Err("A forecast needs one row per period, but the result mixes several series.".into());
    }
    Ok(Series { period: result.columns[period].name.clone(), value: result.columns[value].name.clone() })
}

/// Periods are indexed by their real offset from the first one (not row position), so gaps in the
/// history don't distort the trend. The period step and season length come from the median gap
/// (day → weekly season, week → yearly, month → yearly, quarter → yearly, year → none).
fn sql(history: &str, series: &Series, periods: u32) -> String {
    format!(
        "WITH lb_history AS ({history}), \
         lb_points AS (SELECT CAST({period} AS TIMESTAMP) AS lb_period, CAST({value} AS DOUBLE) AS lb_value FROM lb_history \
           WHERE {period} IS NOT NULL AND {value} IS NOT NULL), \
         lb_gaps AS (SELECT lb_period, lb_value, min(lb_period) OVER () AS lb_first, \
           date_diff('day', lag(lb_period) OVER (ORDER BY lb_period), lb_period) AS lb_gap FROM lb_points), \
         lb_step AS (SELECT median(lb_gap) AS lb_median_gap, \
           CASE WHEN median(lb_gap) <= 1 THEN 7 WHEN median(lb_gap) <= 8 THEN 52 WHEN median(lb_gap) <= 31 THEN 12 \
             WHEN median(lb_gap) <= 92 THEN 4 ELSE 0 END AS lb_season, \
           CASE WHEN median(lb_gap) <= 1 THEN 1 WHEN median(lb_gap) <= 8 THEN 7 ELSE 0 END AS lb_step_days, \
           CASE WHEN median(lb_gap) <= 8 THEN 0 WHEN median(lb_gap) <= 31 THEN 1 WHEN median(lb_gap) <= 92 THEN 3 ELSE 12 END AS lb_step_months \
           FROM lb_gaps), \
         lb_indexed AS (SELECT g.lb_period, g.lb_value, \
           CAST(round(CASE WHEN s.lb_step_months > 0 THEN date_diff('month', g.lb_first, g.lb_period) / CAST(s.lb_step_months AS DOUBLE) \
             ELSE date_diff('day', g.lb_first, g.lb_period) / CAST(s.lb_step_days AS DOUBLE) END) AS BIGINT) AS lb_t \
           FROM lb_gaps g, lb_step s), \
         lb_fit AS (SELECT regr_slope(lb_value, lb_t) AS lb_slope, regr_intercept(lb_value, lb_t) AS lb_intercept, \
           max(lb_t) AS lb_last_t, max(lb_period) AS lb_last_period FROM lb_indexed), \
         lb_shape AS (SELECT f.*, s.lb_season, s.lb_step_days, s.lb_step_months FROM lb_fit f, lb_step s), \
         lb_method AS (SELECT *, lb_season > 0 AND lb_last_t + 1 >= 2 * lb_season AS lb_seasonal FROM lb_shape), \
         lb_future AS (SELECT unnest(generate_series(1, {periods})) AS lb_k, * FROM lb_method) \
         SELECT h.lb_period AS period, h.lb_value AS actual, \
           CASE WHEN h.lb_t = m.lb_last_t THEN h.lb_value END AS {forecast}, CAST(NULL AS VARCHAR) AS method \
         FROM lb_indexed h, lb_method m \
         UNION ALL \
         SELECT f.lb_last_period + to_months(CAST(f.lb_k * f.lb_step_months AS INTEGER)) + to_days(CAST(f.lb_k * f.lb_step_days AS INTEGER)), \
           NULL, \
           CASE WHEN f.lb_seasonal AND s.lb_value IS NOT NULL THEN s.lb_value + f.lb_slope * f.lb_season * ceil(f.lb_k / f.lb_season) \
             ELSE f.lb_intercept + f.lb_slope * (f.lb_last_t + f.lb_k) END, \
           CASE WHEN f.lb_seasonal THEN 'seasonal' ELSE 'linear' END \
         FROM lb_future f LEFT JOIN lb_indexed s \
           ON f.lb_seasonal AND s.lb_t = f.lb_last_t + f.lb_k - f.lb_season * CAST(ceil(f.lb_k / f.lb_season) AS BIGINT) \
         ORDER BY 1",
        period = quote_ident(&series.period),
        value = quote_ident(&series.value),
        forecast = quote_ident(FORECAST_COLUMN),
    )
}

fn projected_rows(result: &QueryResult) -> impl Iterator<Item = &Vec<Value>> {
    result.rows.iter().filter(|row| row.get(METHOD).is_some_and(|method| !method.is_null()))
}

fn method_of(result: &QueryResult) -> &str {
    projected_rows(result).next().and_then(|row| row.get(METHOD)).and_then(Value::as_str).unwrap_or("linear")
}

/// The projected values, for the narration.
pub fn summary(result: &QueryResult) -> Value {
    let points: Vec<Value> = projected_rows(result)
        .map(|row| {
            json!({
                "period": row.get(PERIOD).cloned().unwrap_or(Value::Null),
                "forecast": row.get(FORECAST).and_then(as_f64).map(round),
            })
        })
        .collect();
    json!({ "method": method_of(result), "projected": points })
}

impl Run {
    /// Extends a historical series with projected periods. Returns `None` (with a warning) when the
    /// history can't support a forecast, so the plain history is shown instead.
    pub(super) async fn project(
        &mut self,
        history: &QueryRecord,
        result: &QueryResult,
        periods: u32,
        step: &str,
    ) -> Result<Option<(QueryRecord, QueryResult)>, AnalystError> {
        self.step(step, "Projecting the trend");
        let series = match series(result) {
            Ok(series) => series,
            Err(reason) => {
                self.finish_step(step, StepStatus::Skipped, Some(reason.clone()));
                self.answer.warnings.push(reason);
                return Ok(None);
            }
        };
        let periods = if periods == 0 { DEFAULT_PERIODS } else { periods.min(MAX_PERIODS) };
        match self.execute_once(QueryPurpose::Answer, &sql(&history.sql, &series, periods)).await? {
            Ok((record, projected)) => {
                let method = match method_of(&projected) {
                    "seasonal" => "the same period one season earlier plus the overall trend",
                    _ => "a straight-line trend over the history",
                };
                self.finish_step(step, StepStatus::Done, Some(format!("{periods} periods ahead")));
                self.answer.assumptions.push(format!("The forecast of the next {periods} periods uses {method}."));
                Ok(Some((record, projected)))
            }
            Err(error) => {
                self.finish_step(step, StepStatus::Skipped, Some(error.clone()));
                self.answer.warnings.push(format!("Could not project the trend: {error}"));
                Ok(None)
            }
        }
    }
}

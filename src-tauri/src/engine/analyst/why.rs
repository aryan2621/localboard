//! "Why did X change?": the model only chooses what to compare; the comparison and driver
//! ranking are deterministic SQL built here and every number is computed in code.

use serde_json::{json, Value};

use super::chart::{ChartKind, ChartSpec};
use super::context::{SchemaContext, TableSchema};
use super::facts::{as_f64, round};
use super::prompts::{self, Aggregate, Granularity, WhyPlan};
use super::{AnalystError, AnswerMode, Driver, Run, StepStatus};
use crate::engine::duck::{quote_ident, sql_string};
use crate::engine::query::QueryResult;
use crate::llm::provider::{ChatMessage, Role, Task};
use crate::storage::conversations::QueryPurpose;

const TOP_DRIVERS: usize = 5;
const SEGMENTS_PER_DIMENSION: usize = 10;

struct Plan {
    table: String,
    aggregate: Aggregate,
    measure: Option<String>,
    date_column: String,
    granularity: Granularity,
    dimensions: Vec<String>,
    filters: Vec<(String, String)>,
}

fn column<'a>(table: &'a TableSchema, name: &str) -> Option<&'a str> {
    table.columns.iter().find(|(column, _)| column.eq_ignore_ascii_case(name.trim())).map(|(column, _)| column.as_str())
}

fn validate(plan: WhyPlan, context: &SchemaContext) -> Result<Plan, String> {
    let table = context.table(&plan.table).ok_or_else(|| format!("{} is not one of the relevant tables", plan.table))?;
    let date_column =
        column(table, &plan.date_column).ok_or_else(|| format!("{} has no column {}", table.relation, plan.date_column))?.to_owned();
    let measure = match plan.measure.aggregate {
        Aggregate::Count => None,
        _ => Some(
            column(table, &plan.measure.column)
                .ok_or_else(|| format!("{} has no column {}", table.relation, plan.measure.column))?
                .to_owned(),
        ),
    };
    let mut dimensions = Vec::new();
    for name in &plan.dimensions {
        if let Some(found) = column(table, name) {
            let is_used = found == date_column || Some(found) == measure.as_deref() || dimensions.iter().any(|d: &String| d == found);
            if !is_used {
                dimensions.push(found.to_owned());
            }
        }
    }
    let filters = plan
        .filters
        .iter()
        .map(|filter| {
            column(table, &filter.column)
                .map(|found| (found.to_owned(), filter.value.clone()))
                .ok_or_else(|| format!("{} has no column {}", table.relation, filter.column))
        })
        .collect::<Result<_, _>>()?;
    Ok(Plan {
        table: table.relation.clone(),
        aggregate: plan.measure.aggregate,
        measure,
        date_column,
        granularity: plan.granularity,
        dimensions,
        filters,
    })
}

impl Plan {
    fn unit(&self) -> &'static str {
        match self.granularity {
            Granularity::Week => "week",
            Granularity::Month => "month",
            Granularity::Quarter => "quarter",
            Granularity::Year => "year",
        }
    }

    fn interval(&self) -> &'static str {
        match self.granularity {
            Granularity::Week => "7 days",
            Granularity::Month => "1 month",
            Granularity::Quarter => "3 months",
            Granularity::Year => "1 year",
        }
    }

    fn aggregate(&self, filter: &str) -> String {
        match self.aggregate {
            Aggregate::Sum => format!("SUM(lb_value){filter}"),
            Aggregate::Avg => format!("AVG(lb_value){filter}"),
            Aggregate::Count => format!("COUNT(*){filter}"),
        }
    }

    fn metric_label(&self) -> String {
        match (&self.aggregate, &self.measure) {
            (Aggregate::Count, _) | (_, None) => format!("number of rows in {}", self.table),
            (Aggregate::Sum, Some(measure)) => format!("total {measure} in {}", self.table),
            (Aggregate::Avg, Some(measure)) => format!("average {measure} in {}", self.table),
        }
    }

    /// Shared CTEs: the filtered rows, and the start of the latest complete period in the data
    /// (a trailing period that has not run its full length is left out).
    fn base(&self) -> String {
        let value = match &self.measure {
            Some(measure) => format!("TRY_CAST({} AS DOUBLE)", quote_ident(measure)),
            None => "1".to_owned(),
        };
        let dimensions: String = self
            .dimensions
            .iter()
            .enumerate()
            .map(|(index, dimension)| format!(", CAST({} AS VARCHAR) AS lb_dim_{index}", quote_ident(dimension)))
            .collect();
        let filters: Vec<String> = self
            .filters
            .iter()
            .map(|(column, value)| format!("lower(CAST({} AS VARCHAR)) = lower({})", quote_ident(column), sql_string(value)))
            .collect();
        let condition = if filters.is_empty() { String::new() } else { format!(" WHERE {}", filters.join(" AND ")) };
        format!(
            "WITH base AS (SELECT TRY_CAST({date} AS TIMESTAMP) AS lb_date, {value} AS lb_value{dimensions} FROM {table}{condition}), \
             latest AS (SELECT date_trunc('{unit}', MAX(lb_date)) AS latest_start, CAST(MAX(lb_date) AS DATE) AS latest_day FROM base), \
             bounds AS (SELECT CASE WHEN latest_day >= CAST(latest_start + INTERVAL '{interval}' AS DATE) - 1 THEN latest_start \
             ELSE latest_start - INTERVAL '{interval}' END AS current_start, \
             latest_day < CAST(latest_start + INTERVAL '{interval}' AS DATE) - 1 AS trailing_partial FROM latest)",
            date = quote_ident(&self.date_column),
            table = quote_ident(&self.table),
            unit = self.unit(),
            interval = self.interval(),
        )
    }

    fn window(&self) -> String {
        let interval = self.interval();
        format!("lb_date >= current_start - INTERVAL '{interval}' AND lb_date < current_start + INTERVAL '{interval}'")
    }

    fn comparison_sql(&self) -> String {
        format!(
            "{base} SELECT CASE WHEN lb_date >= current_start THEN 'current' ELSE 'previous' END AS period, \
             CAST(MIN(date_trunc('{unit}', lb_date)) AS DATE) AS period_start, {value} AS value, bool_or(trailing_partial) AS trailing_partial \
             FROM base, bounds WHERE {window} GROUP BY 1 ORDER BY 2",
            base = self.base(),
            unit = self.unit(),
            value = self.aggregate(""),
            window = self.window(),
        )
    }

    fn breakdown_sql(&self, index: usize) -> String {
        format!(
            "{base}, segments AS (SELECT lb_dim_{index} AS segment, {current} AS current_value, {previous} AS previous_value \
             FROM base, bounds WHERE {window} GROUP BY 1) \
             SELECT segment, current_value, previous_value FROM segments \
             ORDER BY abs(coalesce(current_value, 0) - coalesce(previous_value, 0)) DESC LIMIT {SEGMENTS_PER_DIMENSION}",
            base = self.base(),
            current = self.aggregate(" FILTER (WHERE lb_date >= current_start)"),
            previous = self.aggregate(" FILTER (WHERE lb_date < current_start)"),
            window = self.window(),
        )
    }
}

fn skipped_partial(result: &QueryResult) -> bool {
    result.rows.iter().any(|row| row.get(3).and_then(Value::as_bool) == Some(true))
}

/// (start, value) for the previous and current periods, if both have data.
fn periods(result: &QueryResult) -> Option<((Value, f64), (Value, f64))> {
    let find = |label: &str| {
        result.rows.iter().find(|row| row.first().and_then(Value::as_str) == Some(label)).map(|row| {
            (row.get(1).cloned().unwrap_or(Value::Null), row.get(2).and_then(as_f64).unwrap_or(0.0))
        })
    };
    Some((find("previous")?, find("current")?))
}

impl Run {
    /// Returns false when the question can't be answered this way, so the regular path runs instead.
    pub(super) async fn explain_change(&mut self, context: &SchemaContext) -> Result<bool, AnalystError> {
        self.step("why", "Choosing what to compare");
        let relations: Vec<String> = context.tables.iter().map(|table| table.relation.clone()).collect();
        let plan: WhyPlan = self
            .ask_json(
                Task::Reasoning,
                prompts::why_system(&context.prompt, &self.guidance),
                vec![ChatMessage { role: Role::User, content: self.question.clone() }],
                "why_plan",
                prompts::why_schema(&relations),
            )
            .await?;
        let plan = match validate(plan, context) {
            Ok(plan) => plan,
            Err(reason) => {
                self.finish_step("why", StepStatus::Skipped, Some(reason));
                return Ok(false);
            }
        };
        self.finish_step("why", StepStatus::Done, Some(plan.metric_label()));

        self.step("compare", format!("Comparing the latest {} with the one before", plan.unit()));
        let comparison = match self.execute_once(QueryPurpose::Comparison, &plan.comparison_sql()).await? {
            Ok(found) => found,
            Err(error) => {
                self.finish_step("compare", StepStatus::Skipped, Some(error));
                return Ok(false);
            }
        };
        let Some(((previous_start, previous), (current_start, current))) = periods(&comparison.1) else {
            self.finish_step("compare", StepStatus::Skipped, Some("Only one period has data".to_owned()));
            return Ok(false);
        };
        let total_change = current - previous;
        let partial_skipped = skipped_partial(&comparison.1);
        self.finish_step("compare", StepStatus::Done, None);

        let mut drivers = Vec::new();
        let mut biggest: Option<(f64, String)> = None;
        if !plan.dimensions.is_empty() {
            self.step("drivers", format!("Breaking the change down by {}", plan.dimensions.join(", ")));
            for (index, dimension) in plan.dimensions.iter().enumerate() {
                let (record, result) = match self.execute_once(QueryPurpose::Breakdown, &plan.breakdown_sql(index)).await? {
                    Ok(found) => found,
                    Err(error) => {
                        self.answer.warnings.push(format!("Could not break down by {dimension}: {error}"));
                        continue;
                    }
                };
                for row in &result.rows {
                    let now = row.get(1).and_then(as_f64).unwrap_or(0.0);
                    let before = row.get(2).and_then(as_f64).unwrap_or(0.0);
                    let change = now - before;
                    if biggest.as_ref().is_none_or(|(size, _)| change.abs() > *size) {
                        biggest = Some((change.abs(), record.id.clone()));
                    }
                    drivers.push(Driver {
                        dimension: dimension.clone(),
                        segment: row.first().cloned().unwrap_or(Value::Null),
                        previous: round(before),
                        current: round(now),
                        change: round(change),
                        // Segment averages don't add up to the overall average, so no contribution share.
                        share_of_change: (plan.aggregate != Aggregate::Avg && total_change != 0.0)
                            .then(|| round(change / total_change)),
                    });
                }
            }
            drivers.sort_by(|a, b| b.change.abs().total_cmp(&a.change.abs()));
            drivers.truncate(TOP_DRIVERS);
            self.finish_step("drivers", StepStatus::Done, None);
        }

        let format_start = |value: &Value| value.as_str().map_or_else(|| value.to_string(), str::to_owned);
        self.answer.mode = AnswerMode::Why;
        self.answer.explanation = Some(format!(
            "Compared the {} between the two latest {}s{}.",
            plan.metric_label(),
            plan.unit(),
            if plan.dimensions.is_empty() { String::new() } else { format!(" and broke the change down by {}", plan.dimensions.join(", ")) }
        ));
        self.answer.assumptions.push(format!(
            "Compared the {unit} starting {} (the latest in the data) with the {unit} starting {}.",
            format_start(&current_start),
            format_start(&previous_start),
            unit = plan.unit()
        ));
        if partial_skipped {
            self.answer.assumptions.push(format!(
                "The latest {} in the data is incomplete, so it was left out and the two most recent complete {}s were compared.",
                plan.unit(),
                plan.unit()
            ));
        }
        if plan.aggregate == Aggregate::Avg {
            self.answer.assumptions.push(
                "This is an average, so segments are ranked by the size of their own change; they do not add up to the overall change."
                    .to_owned(),
            );
        }
        for (column, value) in &plan.filters {
            self.answer.assumptions.push(format!("Only rows where {column} is {value}."));
        }
        self.answer.primary_query_id = Some(comparison.0.id.clone());
        self.answer.chart = Some(match biggest {
            Some((_, query_id)) => ChartSpec {
                kind: ChartKind::Bar,
                query_id,
                x: Some("segment".to_owned()),
                y: vec!["previous_value".to_owned(), "current_value".to_owned()],
                dashed: Vec::new(),
            },
            None => ChartSpec {
                kind: ChartKind::Bar,
                query_id: comparison.0.id.clone(),
                x: Some("period".to_owned()),
                y: vec!["value".to_owned()],
                dashed: Vec::new(),
            },
        });
        self.answer.drivers = drivers;

        let facts = json!({
            "metric": plan.metric_label(),
            "period": plan.unit(),
            "previous_period_start": previous_start,
            "current_period_start": current_start,
            "previous": round(previous),
            "current": round(current),
            "change": round(total_change),
            "change_pct": (previous != 0.0).then(|| round(100.0 * total_change / previous.abs())),
            "biggest_drivers": self.answer.drivers,
            "assumptions": self.answer.assumptions,
            "warnings": self.answer.warnings,
        });
        self.narrate(facts).await?;
        Ok(true)
    }
}

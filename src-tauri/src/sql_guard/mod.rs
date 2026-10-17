//! Read-only by construction: every statement the analyst (or a user) runs is parsed and checked
//! here before it reaches DuckDB. The checks are allowlists, so anything not explicitly
//! understood is rejected.

use std::collections::{BTreeSet, HashMap};
use std::ops::ControlFlow;

use sqlparser::ast::{Expr, ObjectName, Query, Select, SetExpr, Statement, TableFactor, Value, Visit, Visitor};
use sqlparser::dialect::DuckDbDialect;
use sqlparser::parser::Parser;

/// Upper bound on accepted SQL text, well above anything a model or person writes by hand.
const MAX_SQL_BYTES: usize = 20_000;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct GuardViolation(String);

impl GuardViolation {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// What a query may touch: the workspace's registered relations, by case-insensitive name.
#[derive(Clone)]
pub struct GuardPolicy {
    relations: HashMap<String, String>,
}

impl GuardPolicy {
    pub fn new<I: IntoIterator<Item = String>>(relations: I) -> Self {
        Self { relations: relations.into_iter().map(|name| (name.to_lowercase(), name)).collect() }
    }
}

#[derive(Clone, Debug)]
pub struct ValidatedQuery {
    /// The statement re-rendered from the parsed tree, so comments and trailing text are gone.
    pub sql: String,
    /// Registered relations the query reads, for lineage.
    pub relations: Vec<String>,
    /// Whether the query already bounds its row count (LIMIT or FETCH).
    pub has_limit: bool,
    /// Named parameters (`:region`), without the sigil, in first-seen order.
    pub parameters: Vec<String>,
    /// Every column name the query mentions, with whether it appears only as a bare, unaliased item in
    /// the outermost SELECT list (the one place output masking can follow a column).
    pub columns: Vec<ColumnUse>,
}

#[derive(Clone, Debug)]
pub struct ColumnUse {
    /// Lower-cased column name (last part of a qualified reference).
    pub name: String,
    pub bare_output: bool,
}

struct ColumnCollector {
    bare: std::collections::HashSet<*const Expr>,
    uses: Vec<ColumnUse>,
}

impl Visitor for ColumnCollector {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        let name = match expr {
            Expr::Identifier(ident) => Some(ident.value.to_lowercase()),
            Expr::CompoundIdentifier(parts) => parts.last().map(|ident| ident.value.to_lowercase()),
            _ => None,
        };
        if let Some(name) = name {
            self.uses.push(ColumnUse { name, bare_output: self.bare.contains(&(expr as *const Expr)) });
        }
        ControlFlow::Continue(())
    }
}

fn collect_columns(statement: &Statement) -> Vec<ColumnUse> {
    let mut bare = std::collections::HashSet::new();
    let query = match statement {
        Statement::Query(query) => Some(query),
        _ => None,
    };
    if let Some(query) = query {
        if let SetExpr::Select(select) = &*query.body {
            for item in &select.projection {
                if let sqlparser::ast::SelectItem::UnnamedExpr(expr @ (Expr::Identifier(_) | Expr::CompoundIdentifier(_))) = item {
                    bare.insert(expr as *const Expr);
                }
            }
        }
    }
    let mut collector = ColumnCollector { bare, uses: Vec::new() };
    let _ = statement.visit(&mut collector);
    collector.uses
}

pub fn validate(sql: &str, policy: &GuardPolicy) -> Result<ValidatedQuery, GuardViolation> {
    let text = sql.trim().trim_end_matches(';').trim();
    if text.is_empty() {
        return Err(GuardViolation::new("The query is empty."));
    }
    if text.len() > MAX_SQL_BYTES {
        return Err(GuardViolation::new("The query is too long."));
    }
    let mut statements = Parser::parse_sql(&DuckDbDialect {}, text)
        .map_err(|error| GuardViolation::new(format!("The SQL could not be parsed: {error}")))?;
    if statements.len() != 1 {
        return Err(GuardViolation::new("Only a single statement can be run."));
    }
    let statement = statements.remove(0);
    let (statement, explain) = match statement {
        Statement::Explain { analyze, statement, .. } => {
            if analyze {
                return Err(GuardViolation::new("EXPLAIN ANALYZE is not available. Use EXPLAIN."));
            }
            (*statement, true)
        }
        other => (other, false),
    };
    let Statement::Query(query) = &statement else {
        return Err(GuardViolation::new("Only SELECT queries can be run. LocalBoard never changes your data."));
    };

    let mut checker = Checker { policy, scopes: Vec::new(), relations: BTreeSet::new(), statements: 0, parameters: Vec::new() };
    if let ControlFlow::Break(violation) = statement.visit(&mut checker) {
        return Err(violation);
    }
    let columns = collect_columns(&statement);
    let mut sql = statement.to_string();
    let mut has_limit = query.limit_clause.is_some() || query.fetch.is_some();
    if explain {
        sql = format!("EXPLAIN {sql}");
        has_limit = true;
    }
    Ok(ValidatedQuery { sql, relations: checker.relations.into_iter().collect(), has_limit, parameters: checker.parameters, columns })
}

/// The CTEs one query defines. A name is only treated as a CTE where SQL makes it visible: inside
/// that query, and within the WITH list only to later CTEs (or itself when RECURSIVE). Anywhere
/// else DuckDB would resolve the name on its own, for example as a file path.
struct CteScope {
    names: Vec<String>,
    /// Identity of each CTE's body, to tell when the visitor enters or leaves it.
    bodies: Vec<*const Query>,
    recursive: bool,
    /// How many of `names` are visible at the current point of the walk.
    visible: usize,
}

impl CteScope {
    fn of(query: &Query) -> Self {
        let (names, bodies, recursive) = match &query.with {
            Some(with) => (
                with.cte_tables.iter().map(|cte| cte.alias.name.value.to_lowercase()).collect(),
                with.cte_tables.iter().map(|cte| &*cte.query as *const Query).collect(),
                with.recursive,
            ),
            None => (Vec::new(), Vec::new(), false),
        };
        Self { names, bodies, recursive, visible: 0 }
    }

    fn body_index(&self, query: &Query) -> Option<usize> {
        self.bodies.iter().position(|&body| std::ptr::eq(body, query))
    }
}

struct Checker<'a> {
    policy: &'a GuardPolicy,
    scopes: Vec<CteScope>,
    relations: BTreeSet<String>,
    statements: usize,
    parameters: Vec<String>,
}

impl Checker<'_> {
    fn is_visible_cte(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.names[..scope.visible].iter().any(|cte| cte == name))
    }
}

fn single_part(name: &ObjectName) -> Option<String> {
    match name.0.as_slice() {
        [part] => part.as_ident().map(|ident| ident.value.to_lowercase()),
        _ => None,
    }
}

impl Visitor for Checker<'_> {
    type Break = GuardViolation;

    fn pre_visit_statement(&mut self, _statement: &Statement) -> ControlFlow<GuardViolation> {
        // The root query is the only statement allowed; any nested one (e.g. a data-modifying CTE) is not.
        self.statements += 1;
        if self.statements > 1 {
            return ControlFlow::Break(GuardViolation::new("Statements inside a query are not allowed."));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<GuardViolation> {
        if !query.locks.is_empty() || query.for_clause.is_some() || query.settings.is_some() || query.format_clause.is_some() {
            return ControlFlow::Break(GuardViolation::new("Locking, SETTINGS, FORMAT and FOR clauses are not allowed."));
        }
        if !query.pipe_operators.is_empty() {
            return ControlFlow::Break(GuardViolation::new("Pipe syntax is not supported."));
        }
        if !matches!(*query.body, SetExpr::Select(_) | SetExpr::Query(_) | SetExpr::SetOperation { .. } | SetExpr::Values(_)) {
            return ControlFlow::Break(GuardViolation::new("Only SELECT queries can be run."));
        }
        if let Some(parent) = self.scopes.last_mut() {
            if let Some(index) = parent.body_index(query) {
                parent.visible = if parent.recursive { index + 1 } else { index };
            }
        }
        self.scopes.push(CteScope::of(query));
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, query: &Query) -> ControlFlow<GuardViolation> {
        self.scopes.pop();
        if let Some(parent) = self.scopes.last_mut() {
            if let Some(index) = parent.body_index(query) {
                parent.visible = index + 1;
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<GuardViolation> {
        if select.into.is_some() {
            return ControlFlow::Break(GuardViolation::new("SELECT INTO creates a table and is not allowed."));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<GuardViolation> {
        match factor {
            TableFactor::Table { args: None, version: None, .. }
            | TableFactor::Derived { .. }
            | TableFactor::NestedJoin { .. }
            | TableFactor::UNNEST { .. } => ControlFlow::Continue(()),
            TableFactor::Table { name, .. } | TableFactor::Function { name, .. } => ControlFlow::Break(GuardViolation::new(
                format!("Table functions such as {name}(…) are not allowed; query the registered tables instead."),
            )),
            _ => ControlFlow::Break(GuardViolation::new("This kind of FROM clause is not supported.")),
        }
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<GuardViolation> {
        let Some(name) = single_part(relation) else {
            return ControlFlow::Break(GuardViolation::new(format!(
                "{relation} is not a table in this workspace. Use table names exactly as listed."
            )));
        };
        if self.is_visible_cte(&name) {
            return ControlFlow::Continue(());
        }
        match self.policy.relations.get(&name) {
            Some(canonical) => {
                self.relations.insert(canonical.clone());
                ControlFlow::Continue(())
            }
            None => ControlFlow::Break(GuardViolation::new(format!("There is no table named {relation} in this workspace."))),
        }
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<GuardViolation> {
        if let Expr::Value(value) = expr {
            if let Value::Placeholder(raw) = &value.value {
                match parameter_name(raw) {
                    Ok(name) => {
                        if !self.parameters.iter().any(|existing| existing.eq_ignore_ascii_case(&name)) {
                            self.parameters.push(name);
                        }
                    }
                    Err(violation) => return ControlFlow::Break(violation),
                }
            }
        }
        if let Expr::Function(function) = expr {
            let allowed = single_part(&function.name).is_some_and(|name| ALLOWED_FUNCTIONS.contains(&name.as_str()));
            if !allowed {
                return ControlFlow::Break(GuardViolation::new(format!(
                    "The function {}() is not allowed in LocalBoard queries.",
                    function.name
                )));
            }
        }
        ControlFlow::Continue(())
    }
}

fn parameter_name(raw: &str) -> Result<String, GuardViolation> {
    let name = raw.strip_prefix(':').or_else(|| raw.strip_prefix('$')).unwrap_or(raw);
    let ok = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(GuardViolation::new("Use a named parameter such as :region."));
    }
    Ok(name.to_owned())
}

/// Pure functions over values. Anything that reads files, the network, settings or the
/// environment is deliberately absent.
const ALLOWED_FUNCTIONS: &[&str] = &[
    // Aggregates
    "count", "sum", "avg", "mean", "min", "max", "median", "mode", "quantile", "quantile_cont", "quantile_disc",
    "stddev", "stddev_pop", "stddev_samp", "variance", "var_pop", "var_samp", "string_agg", "group_concat",
    "list", "array_agg", "first", "last", "any_value", "arg_max", "arg_min", "argmax", "argmin", "max_by", "min_by",
    "count_if", "countif", "bool_and", "bool_or", "approx_count_distinct", "approx_quantile", "corr", "covar_pop",
    "covar_samp", "regr_slope", "regr_intercept", "regr_r2", "regr_count", "product", "fsum", "sumkahan", "histogram",
    "entropy", "kurtosis", "skewness", "bit_and", "bit_or", "bit_xor", "percentile_cont", "percentile_disc", "mad",
    "arbitrary", "count_star", "reservoir_quantile", "approx_top_k", "regr_avgx", "regr_avgy", "regr_sxx", "regr_syy",
    "regr_sxy", "columns",
    // Window
    "row_number", "rank", "dense_rank", "percent_rank", "cume_dist", "ntile", "lag", "lead", "first_value",
    "last_value", "nth_value",
    // Math
    "abs", "round", "round_even", "ceil", "ceiling", "floor", "trunc", "sqrt", "cbrt", "power", "pow", "exp", "ln",
    "log", "log10", "log2", "sign", "greatest", "least", "mod", "fmod", "pi", "degrees", "radians", "sin", "cos",
    "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "cot", "isnan", "isinf", "isfinite", "even", "gcd", "lcm", "factorial", "divide",
    // Strings
    "lower", "upper", "lcase", "ucase", "length", "len", "char_length", "character_length", "octet_length", "trim",
    "ltrim", "rtrim", "substr", "substring", "replace", "concat", "concat_ws", "left", "right", "lpad", "rpad",
    "position", "strpos", "instr", "contains", "starts_with", "ends_with", "prefix", "suffix", "split_part",
    "string_split", "str_split", "regexp_matches", "regexp_replace", "regexp_extract", "regexp_full_match", "reverse",
    "repeat", "format", "printf", "strip_accents", "ascii", "chr", "md5", "like_escape", "ilike_escape",
    "levenshtein", "jaro_winkler_similarity", "translate", "regexp_split_to_array", "string_to_array", "nvl", "bit_length",
    "starts_with", "damerau_levenshtein", "hamming", "jaccard", "unicode", "array_to_string", "list_aggregate",
    // Dates and times
    "date", "date_trunc", "datetrunc", "date_part", "datepart", "date_diff", "datediff", "date_sub", "datesub", "date_add",
    "strftime", "strptime", "try_strptime", "make_date", "make_time", "make_timestamp", "current_date",
    "current_timestamp", "now", "today", "year", "month", "day", "quarter", "week", "weekofyear", "yearweek",
    "dayofweek", "dayofmonth", "dayofyear", "isodow", "isoyear", "hour", "minute", "second", "millisecond", "epoch",
    "epoch_ms", "last_day", "monthname", "dayname", "age", "to_timestamp", "to_days", "to_months", "to_years",
    "to_hours", "to_minutes", "to_seconds", "time_bucket", "extract", "to_date", "weekday", "epoch_us", "epoch_ns",
    "make_timestamptz", "timezone", "date_trunc", "dateadd", "microsecond", "decade", "century", "millennium",
    "era", "julian", "strptime", "try_strptime",
    // Conditionals and types
    "coalesce", "nullif", "ifnull", "if", "iff", "typeof", "try_cast",
    // Lists (values only)
    "list_value", "list_extract", "list_contains", "list_sort", "list_distinct", "list_unique", "array_length",
    "unnest", "range", "generate_series", "list_sum", "list_avg", "list_min", "list_max", "list_slice", "list_position", "list_count", "list_median",
    "list_concat", "list_reverse", "array_slice", "array_extract", "array_contains", "len",
];

//! Column masks applied at the query-result boundary.

use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::query::QueryResult;
use crate::error::{AppError, AppResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMask {
    pub id: String,
    pub relation: String,
    pub column_name: String,
    pub strategy: MaskStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaskStrategy {
    Redact,
    Hash,
    Email,
}

#[derive(Clone, Debug, Default)]
pub struct AccessPolicy {
    pub masks: Vec<ColumnMask>,
}

impl AccessPolicy {
    pub fn open() -> Self {
        Self::default()
    }

    pub fn query_masks(&self) -> &[ColumnMask] {
        &self.masks
    }

    /// A masked column may only be selected bare in the outermost SELECT (where the result mask follows
    /// it). Inside expressions, filters, aliases, joins or sub-selects it could leak through a value the
    /// mask never sees, so those queries are refused. `count(*)` and other columns are unaffected.
    pub fn check_columns(&self, relations: &[String], uses: &[crate::sql_guard::ColumnUse]) -> AppResult<()> {
        for mask in self.query_masks() {
            if !relations.iter().any(|relation| relation.eq_ignore_ascii_case(&mask.relation)) {
                continue;
            }
            let name = mask.column_name.to_lowercase();
            if uses.iter().any(|item| item.name == name && !item.bare_output) {
                return Err(AppError::invalid(format!(
                    "{}.{} is masked. You can select it as is, but not filter, sort, rename or compute with it.",
                    mask.relation, mask.column_name
                )));
            }
        }
        Ok(())
    }

}

thread_local! {
    static ACTIVE: RefCell<AccessPolicy> = const { RefCell::new(AccessPolicy { masks: Vec::new() }) };
}

pub fn activate(policy: AccessPolicy) {
    ACTIVE.with(|slot| *slot.borrow_mut() = policy);
}

/// Checks access and masked-column use from the validated query, which carries its own column analysis.
pub fn authorize_query(query: &crate::sql_guard::ValidatedQuery) -> AppResult<()> {
    ACTIVE.with(|slot| {
        let policy = slot.borrow();
        policy.check_columns(&query.relations, &query.columns)
    })
}

pub fn finish(result: &mut QueryResult, relations: &[String]) {
    ACTIVE.with(|slot| apply(result, relations, slot.borrow().query_masks()));
}

pub fn apply(result: &mut QueryResult, relations: &[String], masks: &[ColumnMask]) {
    if masks.is_empty() {
        return;
    }
    for (index, column) in result.columns.iter().enumerate() {
        let Some(mask) = masks.iter().find(|mask| {
            mask.column_name.eq_ignore_ascii_case(&column.name)
                && relations.iter().any(|relation| relation.eq_ignore_ascii_case(&mask.relation))
        }) else {
            continue;
        };
        for row in &mut result.rows {
            if let Some(value) = row.get_mut(index) {
                *value = mask_value(mask.strategy, value);
            }
        }
    }
}

fn mask_value(strategy: MaskStrategy, value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        other => {
            let text = match other {
                Value::String(text) => text.clone(),
                _ => other.to_string(),
            };
            Value::String(mask_text(strategy, &text))
        }
    }
}

fn mask_text(strategy: MaskStrategy, value: &str) -> String {
    match strategy {
        MaskStrategy::Redact => "••••".into(),
        MaskStrategy::Hash => {
            let digest = Sha256::digest(value.as_bytes());
            format!("{:02x}{:02x}{:02x}{:02x}…", digest[0], digest[1], digest[2], digest[3])
        }
        MaskStrategy::Email => mask_email(value),
    }
}

fn mask_email(value: &str) -> String {
    let Some((user, domain)) = value.split_once('@') else {
        return "••••".into();
    };
    let first = user.chars().next().unwrap_or('*');
    format!("{first}***@{domain}")
}

use serde::{Deserialize, Serialize};

use super::catalog;
use super::provider::{LlmError, Task};

pub const SETTINGS_KEY: &str = "llm.routing";

/// Which catalog model each task prefers. `None` means automatic (the best installed fit);
/// all tasks default to automatic so one model stays resident instead of reloading per task.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskRouting {
    pub sql: Option<String>,
    pub reasoning: Option<String>,
    pub narrative: Option<String>,
}

impl TaskRouting {
    pub fn get(&self, task: Task) -> Option<&str> {
        match task {
            Task::Sql => self.sql.as_deref(),
            Task::Reasoning => self.reasoning.as_deref(),
            Task::Narrative => self.narrative.as_deref(),
            Task::Embeddings => None,
        }
    }

    pub fn set(&mut self, task: Task, model_id: Option<String>) -> Result<(), LlmError> {
        if let Some(id) = &model_id {
            if catalog::spec(id).is_none() {
                return Err(LlmError::Invalid(format!("Unknown model: {id}")));
            }
        }
        let slot = match task {
            Task::Sql => &mut self.sql,
            Task::Reasoning => &mut self.reasoning,
            Task::Narrative => &mut self.narrative,
            Task::Embeddings => return Err(LlmError::Invalid("Embedding models are configured with agents (M3)".into())),
        };
        *slot = model_id;
        Ok(())
    }
}

use serde::{Deserialize, Serialize};

use super::provider::{LlmError, Task};

pub const SETTINGS_KEY: &str = "llm.routing";

/// Free-text model name per task. `None` uses the provider's configured model.
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

    pub fn set(&mut self, task: Task, model: Option<String>) -> Result<(), LlmError> {
        let model = model.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty());
        let slot = match task {
            Task::Sql => &mut self.sql,
            Task::Reasoning => &mut self.reasoning,
            Task::Narrative => &mut self.narrative,
            Task::Embeddings => return Err(LlmError::Invalid("Embedding models are configured in Settings, AI".into())),
        };
        *slot = model;
        Ok(())
    }
}

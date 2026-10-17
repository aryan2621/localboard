use rusqlite::params;
use sha2::{Digest, Sha256};

use super::workspace_db::WorkspaceDb;
use crate::engine::mask::{ColumnMask, MaskStrategy};
use crate::error::{AppError, AppResult};

impl WorkspaceDb {
    pub fn checkpoint(&self) -> AppResult<()> {
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }


    pub fn list_masks(&self) -> AppResult<Vec<ColumnMask>> {
        let mut statement = self.conn.prepare("SELECT id, relation, column_name, strategy FROM column_masks ORDER BY relation, column_name")?;
        let rows = statement.query_map([], |row| {
            let strategy = match row.get::<_, String>(3)?.as_str() {
                "hash" => MaskStrategy::Hash,
                "email" => MaskStrategy::Email,
                _ => MaskStrategy::Redact,
            };
            Ok(ColumnMask { id: row.get(0)?, relation: row.get(1)?, column_name: row.get(2)?, strategy })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn upsert_mask(&self, mask: &ColumnMask) -> AppResult<ColumnMask> {
        let strategy = match mask.strategy {
            MaskStrategy::Redact => "redact",
            MaskStrategy::Hash => "hash",
            MaskStrategy::Email => "email",
        };
        self.conn.execute(
            "INSERT INTO column_masks(id, relation, column_name, strategy) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(relation, column_name) DO UPDATE SET strategy = excluded.strategy",
            params![mask.id, mask.relation, mask.column_name, strategy],
        )?;
        self.list_masks()?
            .into_iter()
            .find(|item| item.relation == mask.relation && item.column_name == mask.column_name)
            .ok_or_else(|| AppError::Internal("mask was not saved".into()))
    }

    pub fn delete_mask(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM column_masks WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Mask not found"));
        }
        Ok(())
    }
}

pub fn hash_secret(salt: &str, secret: &str) -> String {
    let mut data = format!("{salt}:{secret}").into_bytes();
    for _ in 0..80_000 {
        data = Sha256::digest(&data).to_vec();
    }
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

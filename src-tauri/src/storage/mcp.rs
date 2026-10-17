use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::workspace_db::WorkspaceDb;
use crate::error::{AppError, AppResult};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerRecord {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolRule {
    pub agent_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub allow: bool,
}

impl WorkspaceDb {
    pub fn list_mcp_servers(&self) -> AppResult<Vec<McpServerRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, transport, command, url, enabled FROM mcp_servers ORDER BY created_at, name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(McpServerRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                transport: row.get(2)?,
                command: row.get(3)?,
                url: row.get(4)?,
                enabled: row.get::<_, i64>(5)? == 1,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn mcp_server(&self, id: &str) -> AppResult<McpServerRecord> {
        self.list_mcp_servers()?.into_iter().find(|server| server.id == id).ok_or_else(|| AppError::not_found("MCP server not found"))
    }

    pub fn upsert_mcp_server(&self, record: &McpServerRecord) -> AppResult<McpServerRecord> {
        self.conn.execute(
            "INSERT INTO mcp_servers(id, name, transport, command, url, enabled) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, transport = excluded.transport,
               command = excluded.command, url = excluded.url, enabled = excluded.enabled",
            params![record.id, record.name, record.transport, record.command, record.url, i64::from(record.enabled)],
        )?;
        self.mcp_server(&record.id)
    }

    pub fn delete_mcp_server(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM mcp_servers WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("MCP server not found"));
        }
        Ok(())
    }

    pub fn list_mcp_rules(&self, agent_id: Option<&str>) -> AppResult<Vec<McpToolRule>> {
        let mut statement = if agent_id.is_some() {
            self.conn.prepare(
                "SELECT agent_id, server_id, tool_name, allow FROM mcp_tool_rules WHERE agent_id = ?1 ORDER BY tool_name",
            )?
        } else {
            self.conn.prepare("SELECT agent_id, server_id, tool_name, allow FROM mcp_tool_rules ORDER BY agent_id, tool_name")?
        };
        let map = |row: &rusqlite::Row<'_>| {
            Ok(McpToolRule {
                agent_id: row.get(0)?,
                server_id: row.get(1)?,
                tool_name: row.get(2)?,
                allow: row.get::<_, i64>(3)? == 1,
            })
        };
        let rows = if let Some(agent_id) = agent_id {
            statement.query_map([agent_id], map)?
        } else {
            statement.query_map([], map)?
        };
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_mcp_rule(&self, rule: &McpToolRule) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO mcp_tool_rules(agent_id, server_id, tool_name, allow) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(agent_id, server_id, tool_name) DO UPDATE SET allow = excluded.allow",
            params![rule.agent_id, rule.server_id, rule.tool_name, i64::from(rule.allow)],
        )?;
        Ok(())
    }

    pub fn delete_mcp_rule(&self, agent_id: &str, server_id: &str, tool_name: &str) -> AppResult<()> {
        self.conn.execute(
            "DELETE FROM mcp_tool_rules WHERE agent_id = ?1 AND server_id = ?2 AND tool_name = ?3",
            params![agent_id, server_id, tool_name],
        )?;
        Ok(())
    }

    pub fn mcp_rule(&self, agent_id: &str, server_id: &str, tool_name: &str) -> AppResult<Option<McpToolRule>> {
        Ok(self
            .conn
            .query_row(
                "SELECT agent_id, server_id, tool_name, allow FROM mcp_tool_rules
                 WHERE agent_id = ?1 AND server_id = ?2 AND tool_name = ?3",
                params![agent_id, server_id, tool_name],
                |row| {
                    Ok(McpToolRule {
                        agent_id: row.get(0)?,
                        server_id: row.get(1)?,
                        tool_name: row.get(2)?,
                        allow: row.get::<_, i64>(3)? == 1,
                    })
                },
            )
            .optional()?)
    }
}

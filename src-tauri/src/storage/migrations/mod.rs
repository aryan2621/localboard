use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const APP: &[Migration] = &[
    Migration { version: 1, name: "init", sql: include_str!("app_0001_init.sql") },
    Migration { version: 2, name: "governance", sql: include_str!("app_0002_governance.sql") },
];

pub const WORKSPACE: &[Migration] = &[
    Migration { version: 1, name: "init", sql: include_str!("workspace_0001_init.sql") },
    Migration { version: 2, name: "analyst", sql: include_str!("workspace_0002_analyst.sql") },
    Migration { version: 3, name: "agents", sql: include_str!("workspace_0003_agents.sql") },
    Migration { version: 4, name: "bench", sql: include_str!("workspace_0004_bench.sql") },
    Migration { version: 5, name: "dashboards", sql: include_str!("workspace_0005_dashboards.sql") },
    Migration { version: 6, name: "automations", sql: include_str!("workspace_0006_automations.sql") },
    Migration { version: 7, name: "lineage", sql: include_str!("workspace_0007_lineage.sql") },
    Migration { version: 8, name: "artifacts", sql: include_str!("workspace_0008_artifacts.sql") },
    Migration { version: 9, name: "apps", sql: include_str!("workspace_0009_apps.sql") },
    Migration { version: 10, name: "sheets", sql: include_str!("workspace_0010_sheets.sql") },
    Migration { version: 11, name: "automations_v2", sql: include_str!("workspace_0011_automations_v2.sql") },
    Migration { version: 12, name: "mcp", sql: include_str!("workspace_0012_mcp.sql") },
    Migration { version: 13, name: "governance", sql: include_str!("workspace_0013_governance.sql") },
    Migration { version: 14, name: "automation_step_kinds", sql: include_str!("workspace_0014_automation_step_kinds.sql") },
];

/// Applies every migration newer than the database's recorded version, each in its own
/// transaction. Refuses to open a database written by a newer LocalBoard.
pub fn apply(conn: &mut Connection, migrations: &[Migration]) -> AppResult<u32> {
    if migrations.windows(2).any(|pair| pair[0].version >= pair[1].version) {
        return Err(AppError::Internal("migrations must be strictly ascending".into()));
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations(
           version INTEGER PRIMARY KEY,
           name TEXT NOT NULL,
           applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         );",
    )?;
    let current: u32 = conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))?;
    let latest = migrations.last().map_or(0, |migration| migration.version);
    if current > latest {
        return Err(AppError::Conflict(format!(
            "This data was written by a newer LocalBoard (schema {current}, this build knows {latest}). Update LocalBoard."
        )));
    }
    for migration in migrations.iter().filter(|migration| migration.version > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        )?;
        tx.commit()?;
    }
    Ok(latest)
}

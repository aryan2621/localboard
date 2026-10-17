-- Step kinds added after automations v2 (MCP tool, webhook, write file, notification, branch) were
-- rejected by the old CHECK, so the table is rebuilt with the full list. Nothing references it.
CREATE TABLE automation_steps_new(
  id TEXT PRIMARY KEY,
  automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
  sort INTEGER NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('sql', 'ask', 'dashboard', 'compare', 'threshold', 'anomaly', 'report', 'email', 'mcp_tool', 'webhook', 'write_file', 'notify', 'branch')),
  config_json TEXT NOT NULL,
  UNIQUE (automation_id, sort)
);

INSERT INTO automation_steps_new(id, automation_id, sort, kind, config_json)
SELECT id, automation_id, sort, kind, config_json FROM automation_steps;

DROP TABLE automation_steps;
ALTER TABLE automation_steps_new RENAME TO automation_steps;

CREATE TABLE automations_v2(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  trigger_kind TEXT NOT NULL CHECK (trigger_kind IN ('cron', 'interval', 'file_change', 'manual', 'webhook')),
  trigger_json TEXT NOT NULL DEFAULT '{}',
  enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  on_change_only INTEGER NOT NULL DEFAULT 1 CHECK (on_change_only IN (0, 1)),
  needs_review INTEGER NOT NULL DEFAULT 0 CHECK (needs_review IN (0, 1)),
  source TEXT NOT NULL DEFAULT 'user' CHECK (source IN ('user', 'ai')),
  confirmed_at TEXT,
  webhook_hash TEXT,
  last_run_at TEXT,
  last_status TEXT,
  last_summary TEXT,
  last_hash TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO automations_v2(
  id, name, trigger_kind, trigger_json, enabled, on_change_only, needs_review, source,
  last_run_at, last_status, last_summary, last_hash, created_at, updated_at
)
SELECT
  id, name, 'interval', '{"intervalSecs":' || interval_secs || '}',
  enabled, on_change_only, 0, 'user',
  last_run_at, last_status, last_summary, last_hash, created_at, updated_at
FROM automations;

CREATE TABLE automation_steps(
  id TEXT PRIMARY KEY,
  automation_id TEXT NOT NULL REFERENCES automations_v2(id) ON DELETE CASCADE,
  sort INTEGER NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('sql', 'ask', 'dashboard', 'compare', 'threshold', 'anomaly', 'report', 'email')),
  config_json TEXT NOT NULL,
  UNIQUE (automation_id, sort)
);

INSERT INTO automation_steps(id, automation_id, sort, kind, config_json)
SELECT
  id, id, 0,
  CASE kind WHEN 'query' THEN 'sql' WHEN 'ask' THEN 'ask' WHEN 'dashboard' THEN 'dashboard' ELSE 'sql' END,
  payload_json
FROM automations;

CREATE TABLE automation_runs_v2(
  id TEXT PRIMARY KEY,
  automation_id TEXT NOT NULL REFERENCES automations_v2(id) ON DELETE CASCADE,
  trigger_kind TEXT,
  status TEXT NOT NULL,
  summary TEXT,
  notified INTEGER NOT NULL DEFAULT 0 CHECK (notified IN (0, 1)),
  started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  finished_at TEXT
);

INSERT INTO automation_runs_v2(id, automation_id, trigger_kind, status, summary, notified, started_at, finished_at)
SELECT id, automation_id, 'interval', status, summary, notified, started_at, finished_at FROM automation_runs;

DROP TABLE automation_runs;
DROP TABLE automations;
ALTER TABLE automations_v2 RENAME TO automations;
ALTER TABLE automation_runs_v2 RENAME TO automation_runs;

CREATE INDEX automation_runs_automation ON automation_runs(automation_id, started_at);
CREATE INDEX automation_steps_automation ON automation_steps(automation_id, sort);

CREATE TABLE automation_step_runs(
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES automation_runs(id) ON DELETE CASCADE,
  step_id TEXT NOT NULL,
  sort INTEGER NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
  summary TEXT,
  facts_json TEXT,
  started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  finished_at TEXT
);

CREATE INDEX automation_step_runs_run ON automation_step_runs(run_id, sort);

CREATE TABLE workspace_settings(
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

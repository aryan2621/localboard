CREATE TABLE automations(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  kind TEXT NOT NULL CHECK (kind IN ('ask', 'dashboard', 'query')),
  payload_json TEXT NOT NULL,
  interval_secs INTEGER NOT NULL CHECK (interval_secs >= 60),
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  on_change_only INTEGER NOT NULL DEFAULT 1 CHECK (on_change_only IN (0, 1)),
  last_run_at TEXT,
  last_status TEXT,
  last_summary TEXT,
  last_hash TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE automation_runs(
  id TEXT PRIMARY KEY,
  automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  summary TEXT,
  notified INTEGER NOT NULL DEFAULT 0 CHECK (notified IN (0, 1)),
  started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  finished_at TEXT
);

CREATE INDEX automation_runs_automation ON automation_runs(automation_id, started_at);

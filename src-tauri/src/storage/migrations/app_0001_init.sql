CREATE TABLE workspaces(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_opened_at TEXT
);

CREATE TABLE app_settings(
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

CREATE TABLE dashboard_folders(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE dashboards(
  id TEXT PRIMARY KEY,
  folder_id TEXT REFERENCES dashboard_folders(id) ON DELETE SET NULL,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  prompt TEXT NOT NULL DEFAULT '',
  filters_json TEXT NOT NULL DEFAULT '[]',
  refresh_secs INTEGER,
  last_params_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE dashboard_tiles(
  id TEXT PRIMARY KEY,
  dashboard_id TEXT NOT NULL REFERENCES dashboards(id) ON DELETE CASCADE,
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  sql TEXT NOT NULL CHECK (length(trim(sql)) > 0),
  chart_json TEXT NOT NULL,
  layout_json TEXT NOT NULL,
  insight TEXT,
  facts_hash TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX dashboard_tiles_dashboard ON dashboard_tiles(dashboard_id);

CREATE TABLE dashboard_versions(
  id TEXT PRIMARY KEY,
  dashboard_id TEXT NOT NULL REFERENCES dashboards(id) ON DELETE CASCADE,
  snapshot_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX dashboard_versions_dashboard ON dashboard_versions(dashboard_id, created_at);

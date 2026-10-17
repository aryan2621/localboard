CREATE TABLE connections(
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  config_json TEXT NOT NULL,
  secret_ref TEXT,
  read_only INTEGER NOT NULL DEFAULT 1 CHECK (read_only = 1),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE catalog_tables(
  id TEXT PRIMARY KEY,
  connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
  source_ref TEXT NOT NULL,
  duck_relation TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL DEFAULT '',
  row_count INTEGER,
  samples_json TEXT,
  profile_error TEXT,
  profiled_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (connection_id, source_ref)
);

CREATE TABLE catalog_columns(
  table_id TEXT NOT NULL REFERENCES catalog_tables(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  data_type TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  profile_json TEXT,
  PRIMARY KEY (table_id, name)
);

CREATE TABLE catalog_relationships(
  id TEXT PRIMARY KEY,
  from_table_id TEXT NOT NULL REFERENCES catalog_tables(id) ON DELETE CASCADE,
  from_column TEXT NOT NULL,
  to_table_id TEXT NOT NULL REFERENCES catalog_tables(id) ON DELETE CASCADE,
  to_column TEXT NOT NULL,
  confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  origin TEXT NOT NULL CHECK (origin IN ('detected', 'user')),
  UNIQUE (from_table_id, from_column, to_table_id, to_column)
);

CREATE TABLE audit_log(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  actor TEXT NOT NULL,
  kind TEXT NOT NULL,
  question TEXT,
  sql TEXT,
  rows INTEGER,
  duration_ms INTEGER,
  status TEXT NOT NULL CHECK (status IN ('ok', 'error', 'denied')),
  detail_json TEXT
);

CREATE INDEX audit_log_ts ON audit_log(ts);

CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN
  SELECT RAISE(ABORT, 'audit_log is append-only');
END;

CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN
  SELECT RAISE(ABORT, 'audit_log is append-only');
END;

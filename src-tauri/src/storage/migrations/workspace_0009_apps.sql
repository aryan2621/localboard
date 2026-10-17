CREATE TABLE query_relations_new(
  kind TEXT NOT NULL CHECK (kind IN ('audit', 'query', 'saved', 'tile', 'automation', 'datasite', 'report', 'deck', 'app', 'embed', 'sheet')),
  ref_id TEXT NOT NULL,
  relation TEXT NOT NULL,
  PRIMARY KEY (kind, ref_id, relation)
);
INSERT INTO query_relations_new(kind, ref_id, relation) SELECT kind, ref_id, relation FROM query_relations;
DROP TABLE query_relations;
ALTER TABLE query_relations_new RENAME TO query_relations;
CREATE INDEX query_relations_relation ON query_relations(relation);

CREATE TABLE workspace_theme(
  id INTEGER PRIMARY KEY CHECK (id = 1),
  brand TEXT NOT NULL DEFAULT '#C96240',
  name TEXT NOT NULL DEFAULT 'LocalBoard'
);
INSERT INTO workspace_theme(id) VALUES (1);

CREATE TABLE workspace_secrets(
  name TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE data_apps(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  prompt TEXT NOT NULL DEFAULT '',
  html TEXT NOT NULL DEFAULT '',
  source_message_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE app_bindings(
  id TEXT PRIMARY KEY,
  app_id TEXT NOT NULL REFERENCES data_apps(id) ON DELETE CASCADE,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  title TEXT NOT NULL DEFAULT '',
  sql TEXT NOT NULL CHECK (length(trim(sql)) > 0),
  saved_query_id TEXT REFERENCES saved_queries(id) ON DELETE SET NULL,
  filters_json TEXT NOT NULL DEFAULT '[]',
  sort INTEGER NOT NULL,
  UNIQUE (app_id, name)
);

CREATE INDEX app_bindings_app ON app_bindings(app_id, sort);

CREATE TABLE embeds(
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('dashboard', 'datasite', 'app')),
  ref_id TEXT NOT NULL,
  hmac TEXT NOT NULL,
  allowed_filters_json TEXT NOT NULL DEFAULT '[]',
  verified_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX embeds_ref ON embeds(kind, ref_id);

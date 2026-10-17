CREATE TABLE agents(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  description TEXT NOT NULL DEFAULT '',
  model_overrides_json TEXT NOT NULL DEFAULT '{}',
  assigned_tables_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Every workspace has one analyst. Further analysts are added by the user.
INSERT INTO agents(id, name, description)
VALUES (
  '00000000-0000-4000-8000-000000000001',
  'Analyst',
  'Answers questions about everything in this workspace.'
);

CREATE TABLE rules(
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  title TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL CHECK (length(trim(body)) > 0),
  table_name TEXT,
  embedding BLOB,
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX rules_agent ON rules(agent_id);

-- History is append-only. eval_score stays empty until evals (M4) can gate a promotion.
CREATE TABLE rule_versions(
  id TEXT PRIMARY KEY,
  rule_id TEXT NOT NULL REFERENCES rules(id) ON DELETE CASCADE,
  snapshot_json TEXT NOT NULL,
  eval_score REAL,
  promoted_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX rule_versions_rule ON rule_versions(rule_id, created_at);

CREATE TABLE examples(
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  question TEXT NOT NULL CHECK (length(trim(question)) > 0),
  sql TEXT NOT NULL CHECK (length(trim(sql)) > 0),
  embedding BLOB,
  content_hash TEXT NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('manual', 'thumbs_up')),
  message_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX examples_agent ON examples(agent_id);
CREATE INDEX examples_message ON examples(message_id);

CREATE TABLE memories(
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  body TEXT NOT NULL CHECK (length(trim(body)) > 0),
  status TEXT NOT NULL CHECK (status IN ('proposed', 'active', 'archived')),
  embedding BLOB,
  content_hash TEXT NOT NULL,
  source_message_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX memories_agent ON memories(agent_id, status);

CREATE TABLE knowledge_files(
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX knowledge_files_agent ON knowledge_files(agent_id);

CREATE TABLE knowledge_chunks(
  id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  body TEXT NOT NULL CHECK (length(trim(body)) > 0),
  embedding BLOB,
  content_hash TEXT NOT NULL
);

CREATE INDEX knowledge_chunks_agent ON knowledge_chunks(agent_id);

CREATE TABLE saved_queries(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  sql TEXT NOT NULL CHECK (length(trim(sql)) > 0),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE query_history(
  id TEXT PRIMARY KEY,
  sql TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL CHECK (status IN ('ok', 'error', 'denied')),
  error TEXT,
  row_count INTEGER,
  duration_ms INTEGER,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX query_history_created ON query_history(created_at);

CREATE TABLE eval_cases(
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  question TEXT NOT NULL CHECK (length(trim(question)) > 0),
  expected_sql TEXT,
  expected_result_json TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  CHECK (
    (expected_sql IS NOT NULL AND length(trim(expected_sql)) > 0 AND expected_result_json IS NULL)
    OR (expected_result_json IS NOT NULL AND length(trim(expected_result_json)) > 0 AND expected_sql IS NULL)
  )
);

CREATE INDEX eval_cases_agent ON eval_cases(agent_id, created_at);

CREATE TABLE eval_runs(
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  model_id TEXT NOT NULL,
  prompt_hash TEXT NOT NULL,
  pass_count INTEGER NOT NULL,
  case_count INTEGER NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT
);

CREATE INDEX eval_runs_agent ON eval_runs(agent_id, started_at);

CREATE TABLE eval_results(
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
  case_id TEXT NOT NULL REFERENCES eval_cases(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('pass', 'fail', 'error')),
  actual_sql TEXT,
  diff_json TEXT NOT NULL
);

CREATE INDEX eval_results_run ON eval_results(run_id);

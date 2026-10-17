CREATE TABLE local_users(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL CHECK (role IN ('admin', 'analyst', 'viewer')),
  password_hash TEXT NOT NULL,
  password_salt TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE connection_grants(
  user_id TEXT NOT NULL REFERENCES local_users(id) ON DELETE CASCADE,
  connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
  PRIMARY KEY (user_id, connection_id)
);

CREATE TABLE table_grants(
  user_id TEXT NOT NULL REFERENCES local_users(id) ON DELETE CASCADE,
  table_id TEXT NOT NULL REFERENCES catalog_tables(id) ON DELETE CASCADE,
  PRIMARY KEY (user_id, table_id)
);

CREATE TABLE column_masks(
  id TEXT PRIMARY KEY,
  relation TEXT NOT NULL,
  column_name TEXT NOT NULL,
  strategy TEXT NOT NULL CHECK (strategy IN ('redact', 'hash', 'email')),
  UNIQUE (relation, column_name)
);

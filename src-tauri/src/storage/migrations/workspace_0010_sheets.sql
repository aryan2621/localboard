CREATE TABLE sheets(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  workbook_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE sheet_ranges(
  id TEXT PRIMARY KEY,
  sheet_id TEXT NOT NULL REFERENCES sheets(id) ON DELETE CASCADE,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  address TEXT NOT NULL CHECK (length(trim(address)) > 0),
  saved_query_id TEXT REFERENCES saved_queries(id) ON DELETE SET NULL,
  sql TEXT NOT NULL CHECK (length(trim(sql)) > 0),
  sort INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX sheet_ranges_sheet ON sheet_ranges(sheet_id, sort);

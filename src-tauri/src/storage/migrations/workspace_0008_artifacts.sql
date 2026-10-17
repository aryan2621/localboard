CREATE TABLE query_relations_new(
  kind TEXT NOT NULL CHECK (kind IN ('audit', 'query', 'saved', 'tile', 'automation', 'datasite', 'report', 'deck')),
  ref_id TEXT NOT NULL,
  relation TEXT NOT NULL,
  PRIMARY KEY (kind, ref_id, relation)
);
INSERT INTO query_relations_new(kind, ref_id, relation) SELECT kind, ref_id, relation FROM query_relations;
DROP TABLE query_relations;
ALTER TABLE query_relations_new RENAME TO query_relations;
CREATE INDEX query_relations_relation ON query_relations(relation);

CREATE TABLE datasites(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  prompt TEXT NOT NULL DEFAULT '',
  source_message_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE datasite_pages(
  id TEXT PRIMARY KEY,
  datasite_id TEXT NOT NULL REFERENCES datasites(id) ON DELETE CASCADE,
  slug TEXT NOT NULL CHECK (length(trim(slug)) > 0),
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  sort INTEGER NOT NULL,
  narrative TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (datasite_id, slug)
);

CREATE TABLE datasite_charts(
  id TEXT PRIMARY KEY,
  page_id TEXT NOT NULL REFERENCES datasite_pages(id) ON DELETE CASCADE,
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  sql TEXT NOT NULL CHECK (length(trim(sql)) > 0),
  chart_json TEXT NOT NULL,
  result_json TEXT,
  facts_json TEXT,
  sort INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX datasite_pages_site ON datasite_pages(datasite_id, sort);
CREATE INDEX datasite_charts_page ON datasite_charts(page_id, sort);

CREATE TABLE reports(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  prompt TEXT NOT NULL DEFAULT '',
  theme_json TEXT NOT NULL DEFAULT '{}',
  source_message_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE report_blocks(
  id TEXT PRIMARY KEY,
  report_id TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('narrative', 'chart', 'table')),
  title TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '',
  sql TEXT,
  chart_json TEXT,
  result_json TEXT,
  facts_json TEXT,
  sort INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX report_blocks_report ON report_blocks(report_id, sort);

CREATE TABLE decks(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  prompt TEXT NOT NULL DEFAULT '',
  outline_json TEXT NOT NULL DEFAULT '[]',
  source_message_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE deck_slides(
  id TEXT PRIMARY KEY,
  deck_id TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  body TEXT NOT NULL DEFAULT '',
  sql TEXT,
  chart_json TEXT,
  result_json TEXT,
  sort INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX deck_slides_deck ON deck_slides(deck_id, sort);

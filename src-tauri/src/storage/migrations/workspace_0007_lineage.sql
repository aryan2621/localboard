CREATE TABLE query_relations(
  kind TEXT NOT NULL CHECK (kind IN ('audit', 'query', 'saved', 'tile', 'automation')),
  ref_id TEXT NOT NULL,
  relation TEXT NOT NULL,
  PRIMARY KEY (kind, ref_id, relation)
);

CREATE INDEX query_relations_relation ON query_relations(relation);

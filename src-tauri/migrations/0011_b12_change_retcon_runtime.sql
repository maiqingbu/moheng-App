-- B12 additive change/retcon runtime storage.
-- Migrations 0001-0010 are frozen and must remain byte-identical.

CREATE TABLE IF NOT EXISTS impact_items (
  project_id TEXT NOT NULL,
  impact_set_id TEXT NOT NULL,
  item_ref TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  dependency_type TEXT NOT NULL,
  impact_class TEXT NOT NULL CHECK (impact_class IN ('HARD','REVIEW_REQUIRED','SOFT','EVIDENCE')),
  depth INTEGER NOT NULL CHECK (depth >= 0),
  path_json TEXT NOT NULL DEFAULT '[]',
  first_edge_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (project_id, impact_set_id, item_ref),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (impact_set_id) REFERENCES impact_sets(impact_set_id)
);

CREATE TABLE IF NOT EXISTS impact_frontier (
  project_id TEXT NOT NULL,
  impact_set_id TEXT NOT NULL,
  frontier_seq INTEGER NOT NULL,
  ref TEXT NOT NULL,
  depth INTEGER NOT NULL CHECK (depth >= 0),
  path_json TEXT NOT NULL DEFAULT '[]',
  next_edge_id TEXT,
  processed INTEGER NOT NULL DEFAULT 0 CHECK (processed IN (0,1)),
  created_at TEXT NOT NULL,
  PRIMARY KEY (project_id, impact_set_id, frontier_seq),
  UNIQUE (project_id, impact_set_id, ref),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (impact_set_id) REFERENCES impact_sets(impact_set_id)
);

CREATE INDEX IF NOT EXISTS idx_b12_impact_items_set_depth
  ON impact_items(project_id, impact_set_id, depth, item_ref);
CREATE INDEX IF NOT EXISTS idx_b12_impact_frontier_pending
  ON impact_frontier(project_id, impact_set_id, processed, frontier_seq);

CREATE TRIGGER IF NOT EXISTS b12_dependency_summary_insert
AFTER INSERT ON dependency_edges
BEGIN
  UPDATE project_settings
  SET setting_value=json_set(setting_value, '$.mutationEpoch', COALESCE(json_extract(setting_value, '$.mutationEpoch'), 0) + 1)
  WHERE project_id=NEW.project_id AND setting_key='b12.dependencySummary.global';
END;

CREATE TRIGGER IF NOT EXISTS b12_dependency_summary_update
AFTER UPDATE OF source_ref, target_ref, impact_class, source_hash ON dependency_edges
BEGIN
  UPDATE project_settings
  SET setting_value=json_set(setting_value, '$.mutationEpoch', COALESCE(json_extract(setting_value, '$.mutationEpoch'), 0) + 1)
  WHERE project_id=NEW.project_id AND setting_key='b12.dependencySummary.global';
END;

CREATE TRIGGER IF NOT EXISTS b12_dependency_summary_delete
AFTER DELETE ON dependency_edges
BEGIN
  UPDATE project_settings
  SET setting_value=json_set(setting_value, '$.mutationEpoch', COALESCE(json_extract(setting_value, '$.mutationEpoch'), 0) + 1)
  WHERE project_id=OLD.project_id AND setting_key='b12.dependencySummary.global';
END;

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only)
VALUES
  ('impact_item', 'impact_items', 'B12::ChangeRetconOwner', 0),
  ('impact_frontier', 'impact_frontier', 'B12::ChangeRetconOwner', 0);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class)
VALUES
  ('impact_items', 'change', 'impact_item', 'POLICY'),
  ('impact_frontier', 'change', 'impact_frontier', 'POLICY');

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy)
VALUES
  ('impact_items', 'COPY_REMAP'),
  ('impact_frontier', 'COPY_REMAP');

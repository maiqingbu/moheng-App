-- B-R2-03 R5: a chapter may commit before its post-commit B09 planning rebuild.
-- Keep this prerequisite across generic Health refresh/open/retry paths until
-- the B09 projection reaches the committed revision.
CREATE TABLE IF NOT EXISTS health_b09_rebuild_pending (
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  required_revision INTEGER NOT NULL CHECK (required_revision >= 0),
  reason TEXT NOT NULL,
  marked_at TEXT NOT NULL,
  PRIMARY KEY (project_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE INDEX IF NOT EXISTS idx_health_b09_pending_project_revision
  ON health_b09_rebuild_pending(project_id, required_revision, volume_id);

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only)
VALUES ('health_b09_rebuild_pending', 'health_b09_rebuild_pending', 'B08::B09ProjectionCoordinator', 1);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class)
VALUES ('health_b09_rebuild_pending', 'health_projection', 'health_b09_rebuild_pending', 'POLICY');

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy)
VALUES ('health_b09_rebuild_pending', 'COPY_REMAP');

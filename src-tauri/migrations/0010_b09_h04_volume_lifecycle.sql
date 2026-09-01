-- B09/H04 additive volume lifecycle projection and audit records.
-- Migrations 0001-0009 are frozen; this migration is the first B09 schema.

CREATE TABLE IF NOT EXISTS volume_planning_state_projection (
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  projection_revision INTEGER NOT NULL CHECK (projection_revision >= 0),
  state_digest TEXT NOT NULL CHECK (length(state_digest) = 64),
  projection_digest TEXT NOT NULL CHECK (length(projection_digest) = 64),
  payload_json TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS volume_route_replans (
  replan_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  previous_artifact_id TEXT,
  previous_version INTEGER,
  new_artifact_id TEXT NOT NULL,
  new_version INTEGER NOT NULL CHECK (new_version > 0),
  trigger TEXT NOT NULL,
  reason TEXT NOT NULL,
  source_revision INTEGER NOT NULL CHECK (source_revision >= 0),
  input_refs_json TEXT NOT NULL,
  validation_status TEXT NOT NULL CHECK (validation_status IN ('VALIDATED','REJECTED')),
  project_command_receipt_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (replan_id),
  UNIQUE (project_id, replan_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS volume_close_records (
  close_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  exit_artifact_id TEXT NOT NULL,
  exit_version INTEGER NOT NULL CHECK (exit_version > 0),
  close_revision INTEGER NOT NULL CHECK (close_revision >= 0),
  status TEXT NOT NULL CHECK (status IN ('CLOSED','REJECTED')),
  next_volume_eligibility TEXT NOT NULL CHECK (next_volume_eligibility IN ('ELIGIBLE','BLOCKED')),
  blocking_refs_json TEXT NOT NULL,
  evidence_refs_json TEXT NOT NULL,
  project_command_receipt_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (close_id),
  UNIQUE (project_id, close_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS volume_handoffs (
  handoff_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  from_volume_id TEXT NOT NULL,
  to_volume_id TEXT NOT NULL,
  from_exit_artifact_id TEXT NOT NULL,
  input_refs_json TEXT NOT NULL,
  eligibility TEXT NOT NULL CHECK (eligibility IN ('ELIGIBLE','BLOCKED')),
  project_command_receipt_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (handoff_id),
  UNIQUE (project_id, handoff_id),
  UNIQUE (project_id, from_volume_id, to_volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (from_volume_id) REFERENCES volumes(volume_id),
  FOREIGN KEY (to_volume_id) REFERENCES volumes(volume_id)
);

CREATE INDEX IF NOT EXISTS idx_b09_planning_projection_project_revision
  ON volume_planning_state_projection(project_id, projection_revision, volume_id);
CREATE INDEX IF NOT EXISTS idx_b09_route_replans_project_volume
  ON volume_route_replans(project_id, volume_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_b09_volume_close_project_volume
  ON volume_close_records(project_id, volume_id, close_revision DESC);
CREATE INDEX IF NOT EXISTS idx_b09_volume_handoffs_project_from
  ON volume_handoffs(project_id, from_volume_id, created_at DESC);

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only)
VALUES ('volume_planning_state_projection', 'volume_planning_state_projection', 'B09::PlanningStateProjectionOwner', 1);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class)
VALUES
  ('volume_planning_state_projection', 'planning_projection', 'volume_planning_state_projection', 'POLICY'),
  ('volume_route_replans', 'planning_audit', 'volume', 'AUDIT'),
  ('volume_close_records', 'planning_audit', 'volume', 'AUDIT'),
  ('volume_handoffs', 'planning_audit', 'volume', 'AUDIT');

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy)
VALUES
  ('volume_planning_state_projection', 'COPY_REMAP'),
  ('volume_route_replans', 'COPY_REMAP'),
  ('volume_close_records', 'COPY_REMAP'),
  ('volume_handoffs', 'COPY_REMAP');

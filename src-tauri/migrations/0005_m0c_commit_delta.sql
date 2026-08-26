-- M0C: authoritative storage for CommitBundle V2 decision and gate deltas.
-- This migration is additive; B01 migrations 0001-0004 remain immutable.


CREATE TABLE IF NOT EXISTS state_deltas (
  state_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  state_type TEXT NOT NULL,
  value_json TEXT NOT NULL DEFAULT '{}',
  chapter_id TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (state_id),
  UNIQUE (project_id, state_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE INDEX IF NOT EXISTS idx_state_deltas_project_type
  ON state_deltas(project_id, state_type, updated_at);

CREATE TABLE IF NOT EXISTS decisions (
  decision_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  decision_type TEXT NOT NULL,
  outcome TEXT NOT NULL,
  reason TEXT,
  source_ref TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (decision_id),
  UNIQUE (project_id, decision_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS gate_certificates (
  certificate_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  gate_policy_artifact_id TEXT NOT NULL,
  gate_policy_version INTEGER NOT NULL,
  gate_policy_payload_hash TEXT NOT NULL CHECK (length(gate_policy_payload_hash) = 64),
  evidence_refs_json TEXT NOT NULL DEFAULT '[]',
  source_digest TEXT NOT NULL CHECK (length(source_digest) = 64),
  passed_at TEXT NOT NULL,
  certificate_digest TEXT NOT NULL CHECK (length(certificate_digest) = 64),
  PRIMARY KEY (certificate_id),
  UNIQUE (project_id, certificate_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (gate_policy_artifact_id, gate_policy_version) REFERENCES artifacts(artifact_id, version)
);

CREATE INDEX IF NOT EXISTS idx_decisions_project_status
  ON decisions(project_id, decision_type, outcome, updated_at);
CREATE INDEX IF NOT EXISTS idx_gate_certificates_project_time
  ON gate_certificates(project_id, passed_at);

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only) VALUES
  ('decision', 'decisions', 'CommitService', 0),
  ('state_delta', 'state_deltas', 'CommitService', 0),
  ('gate_certificate', 'gate_certificates', 'CommitService', 0);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class) VALUES
  ('decisions', 'decision', 'decision', 'PERMANENT'),
  ('state_deltas', 'state', 'state_delta', 'PERMANENT'),
  ('gate_certificates', 'receipt', 'gate_certificate', 'PERMANENT');

UPDATE receipt_taxonomy
SET authority_state = 'ACTIVE', owner = 'CommitService'
WHERE receipt_type = 'GateCertificate';

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy) VALUES
  ('state_deltas', 'COPY_REMAP'),
  ('decisions', 'COPY_REMAP'),
  ('gate_certificates', 'COPY_REMAP');

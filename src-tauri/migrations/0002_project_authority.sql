CREATE TABLE IF NOT EXISTS project_meta (
  project_id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  maintenance_state TEXT NOT NULL DEFAULT 'READY' CHECK (maintenance_state IN ('READY', 'MAINTENANCE', 'RECOVERY')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS migration_history (
  version INTEGER PRIMARY KEY NOT NULL,
  applied_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS artifacts (
  artifact_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  artifact_type TEXT NOT NULL,
  scope_level TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  version INTEGER NOT NULL CHECK (version > 0),
  schema_version TEXT NOT NULL,
  artifact_status TEXT NOT NULL CHECK (artifact_status IN ('CANDIDATE', 'VALIDATED', 'COMMITTED', 'STALE', 'REJECTED', 'SUPERSEDED')),
  semantic_lifecycle TEXT,
  authority_level TEXT,
  payload_hash TEXT NOT NULL CHECK (length(payload_hash) = 64),
  provenance_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  committed_at TEXT,
  PRIMARY KEY (artifact_id, version),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);
CREATE TABLE IF NOT EXISTS artifact_payloads (
  artifact_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  payload_json TEXT NOT NULL,
  payload_hash TEXT NOT NULL,
  PRIMARY KEY (artifact_id, version),
  FOREIGN KEY (artifact_id, version) REFERENCES artifacts(artifact_id, version)
);
CREATE TABLE IF NOT EXISTS artifact_dependencies (
  source_artifact_id TEXT NOT NULL,
  source_version INTEGER NOT NULL,
  target_artifact_id TEXT NOT NULL,
  target_version INTEGER NOT NULL,
  target_payload_hash TEXT NOT NULL,
  PRIMARY KEY (source_artifact_id, source_version, target_artifact_id, target_version),
  FOREIGN KEY (source_artifact_id, source_version) REFERENCES artifacts(artifact_id, version),
  FOREIGN KEY (target_artifact_id, target_version) REFERENCES artifacts(artifact_id, version)
);
CREATE TABLE IF NOT EXISTS canonical_heads (
  project_id TEXT NOT NULL,
  artifact_type TEXT NOT NULL,
  scope_level TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  artifact_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  payload_hash TEXT NOT NULL,
  PRIMARY KEY (project_id, artifact_type, scope_level, scope_key),
  FOREIGN KEY (artifact_id, version) REFERENCES artifacts(artifact_id, version)
);
CREATE TABLE IF NOT EXISTS commands (
  project_id TEXT NOT NULL,
  command_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  request_digest TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('PREPARED', 'COMMITTED', 'FAILED')),
  created_at TEXT NOT NULL,
  PRIMARY KEY (project_id, command_id),
  UNIQUE (project_id, idempotency_key)
);
CREATE TABLE IF NOT EXISTS commit_receipts (
  receipt_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  commit_id TEXT NOT NULL,
  command_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  before_revision INTEGER NOT NULL,
  after_revision INTEGER NOT NULL,
  bundle_digest TEXT NOT NULL,
  receipt_json TEXT NOT NULL,
  receipt_digest TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  UNIQUE (project_id, idempotency_key),
  FOREIGN KEY (project_id, command_id) REFERENCES commands(project_id, command_id)
);
CREATE INDEX IF NOT EXISTS idx_artifacts_scope ON artifacts(project_id, artifact_type, scope_level, scope_key, artifact_status, version);
CREATE INDEX IF NOT EXISTS idx_dependencies_source ON artifact_dependencies(source_artifact_id, source_version);
CREATE INDEX IF NOT EXISTS idx_dependencies_target ON artifact_dependencies(target_artifact_id, target_version);

CREATE TABLE IF NOT EXISTS authority_registry (
  authority_key TEXT PRIMARY KEY NOT NULL,
  authoritative_table TEXT NOT NULL,
  write_owner TEXT NOT NULL,
  projection_only INTEGER NOT NULL DEFAULT 0 CHECK (projection_only IN (0, 1))
);
CREATE TABLE IF NOT EXISTS table_family_registry (
  table_name TEXT PRIMARY KEY NOT NULL,
  family TEXT NOT NULL,
  authority_key TEXT NOT NULL,
  retention_class TEXT NOT NULL,
  FOREIGN KEY (authority_key) REFERENCES authority_registry(authority_key)
);
CREATE TABLE IF NOT EXISTS receipt_taxonomy (
  receipt_type TEXT PRIMARY KEY NOT NULL CHECK (receipt_type IN ('CommitReceipt', 'DomainReceipt', 'GateCertificate')),
  authority_state TEXT NOT NULL CHECK (authority_state IN ('ACTIVE', 'RESERVED')),
  owner TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS retention_policies (
  artifact_type TEXT PRIMARY KEY NOT NULL,
  candidate_ttl_days INTEGER,
  keep_committed INTEGER NOT NULL DEFAULT 1 CHECK (keep_committed IN (0, 1)),
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gc_receipts (
  receipt_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  policy_digest TEXT NOT NULL,
  deleted_candidate_count INTEGER NOT NULL CHECK (deleted_candidate_count >= 0),
  created_at TEXT NOT NULL,
  receipt_digest TEXT NOT NULL
);

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only) VALUES
  ('project', 'project_meta', 'ProjectManager', 0),
  ('artifact', 'artifacts', 'CommitService', 0),
  ('artifact_payload', 'artifact_payloads', 'CommitService', 0),
  ('artifact_lineage', 'artifact_dependencies', 'CommitService', 0),
  ('canonical_head', 'canonical_heads', 'CommitService', 0),
  ('command', 'commands', 'CommitService', 0),
  ('commit_receipt', 'commit_receipts', 'CommitService', 0),
  ('retention', 'retention_policies', 'MaintenanceService', 0),
  ('gc_receipt', 'gc_receipts', 'MaintenanceService', 0),
  ('reader_desire', 'reader_desire_ledger', 'M3B', 1),
  ('quality_vector', 'narrative_quality_reports', 'M3B', 1),
  ('production_budget', 'chapter_production_budgets', 'M3B', 0);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class) VALUES
  ('project_meta', 'project', 'project', 'PERMANENT'),
  ('artifacts', 'artifact', 'artifact', 'POLICY'),
  ('artifact_payloads', 'artifact', 'artifact_payload', 'POLICY'),
  ('artifact_dependencies', 'dependency', 'artifact_lineage', 'PERMANENT'),
  ('canonical_heads', 'artifact', 'canonical_head', 'PERMANENT'),
  ('commands', 'commit', 'command', 'AUDIT'),
  ('commit_receipts', 'receipt', 'commit_receipt', 'PERMANENT'),
  ('retention_policies', 'maintenance', 'retention', 'PERMANENT'),
  ('gc_receipts', 'receipt', 'gc_receipt', 'PERMANENT'),
  ('reader_desire_ledger', 'narrative_control', 'reader_desire', 'POLICY'),
  ('narrative_quality_reports', 'quality_evidence', 'quality_vector', 'POLICY'),
  ('chapter_production_budgets', 'runtime_policy', 'production_budget', 'AUDIT');

INSERT OR IGNORE INTO receipt_taxonomy(receipt_type, authority_state, owner) VALUES
  ('CommitReceipt', 'ACTIVE', 'CommitService'),
  ('DomainReceipt', 'RESERVED', 'M0B'),
  ('GateCertificate', 'RESERVED', 'M0B');

CREATE TRIGGER IF NOT EXISTS artifact_payloads_immutable
BEFORE UPDATE ON artifact_payloads
BEGIN
  SELECT RAISE(ABORT, 'artifact payload versions are immutable');
END;

CREATE TRIGGER IF NOT EXISTS committed_artifact_digest_immutable
BEFORE UPDATE OF payload_hash, schema_version, artifact_type, scope_level, scope_key, provenance_json
ON artifacts
WHEN OLD.artifact_status = 'COMMITTED'
BEGIN
  SELECT RAISE(ABORT, 'committed artifact metadata is immutable');
END;

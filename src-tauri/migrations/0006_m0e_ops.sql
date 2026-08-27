-- M0E: rebuildable search plus SQLite operations authority.

CREATE TABLE IF NOT EXISTS search_documents (
  project_id TEXT NOT NULL,
  doc_id TEXT NOT NULL,
  chapter_id TEXT,
  volume_id TEXT,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  kind TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, doc_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE VIRTUAL TABLE IF NOT EXISTS search_documents_fts USING fts5(
  project_id UNINDEXED,
  doc_id UNINDEXED,
  chapter_id UNINDEXED,
  volume_id UNINDEXED,
  title,
  body,
  kind UNINDEXED,
  tokenize='unicode61'
);

CREATE TABLE IF NOT EXISTS b04_runtime_profiles (
  project_id TEXT PRIMARY KEY NOT NULL,
  profile_version TEXT NOT NULL,
  page_size INTEGER NOT NULL,
  auto_vacuum INTEGER NOT NULL,
  mmap_size INTEGER NOT NULL,
  busy_timeout_ms INTEGER NOT NULL,
  synchronous TEXT NOT NULL,
  wal_autocheckpoint INTEGER NOT NULL,
  checkpoint_policy TEXT NOT NULL,
  recorded_at TEXT NOT NULL,
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS wal_governance_reports (
  report_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  wal_bytes INTEGER NOT NULL DEFAULT 0,
  checkpoint_duration_ms INTEGER NOT NULL DEFAULT 0,
  busy_readers INTEGER NOT NULL DEFAULT 0,
  checkpointed_pages INTEGER NOT NULL DEFAULT 0,
  threshold_state TEXT NOT NULL,
  created_at TEXT NOT NULL,
  report_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS vacuum_reports (
  report_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  freelist_before INTEGER NOT NULL DEFAULT 0,
  freelist_after INTEGER NOT NULL DEFAULT 0,
  page_count INTEGER NOT NULL DEFAULT 0,
  mode TEXT NOT NULL CHECK (mode IN ('CHECK','INCREMENTAL','FULL')),
  maintenance_required INTEGER NOT NULL DEFAULT 0 CHECK (maintenance_required IN (0,1)),
  created_at TEXT NOT NULL,
  report_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS backup_manifests (
  backup_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  backup_path TEXT NOT NULL,
  checksum_sha256 TEXT NOT NULL CHECK (length(checksum_sha256) = 64),
  profile_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS integrity_runs (
  integrity_run_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('PASS','FAIL')),
  failure_count INTEGER NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
  report_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE INDEX IF NOT EXISTS idx_search_documents_project_updated
  ON search_documents(project_id, updated_at, doc_id);
CREATE INDEX IF NOT EXISTS idx_wal_reports_project_time
  ON wal_governance_reports(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_vacuum_reports_project_time
  ON vacuum_reports(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_backup_manifests_project_time
  ON backup_manifests(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_integrity_runs_project_time
  ON integrity_runs(project_id, created_at);

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only) VALUES
  ('search_documents', 'search_documents', 'SearchService', 1),
  ('sqlite_runtime_profile', 'b04_runtime_profiles', 'SQLiteOps', 0),
  ('wal_governance_report', 'wal_governance_reports', 'SQLiteOps', 0),
  ('vacuum_report', 'vacuum_reports', 'SQLiteOps', 0),
  ('backup_manifest', 'backup_manifests', 'BackupService', 0),
  ('integrity_run', 'integrity_runs', 'IntegrityService', 0);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class) VALUES
  ('search_documents', 'search_projection', 'search_documents', 'POLICY'),
  ('b04_runtime_profiles', 'sqlite_ops', 'sqlite_runtime_profile', 'AUDIT'),
  ('wal_governance_reports', 'sqlite_ops', 'wal_governance_report', 'AUDIT'),
  ('vacuum_reports', 'sqlite_ops', 'vacuum_report', 'AUDIT'),
  ('backup_manifests', 'backup', 'backup_manifest', 'PERMANENT'),
  ('integrity_runs', 'integrity', 'integrity_run', 'AUDIT');

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy) VALUES
  ('search_documents', 'COPY_CLEAR'),
  ('search_documents_fts', 'COPY_CLEAR'),
  ('b04_runtime_profiles', 'COPY_CLEAR'),
  ('wal_governance_reports', 'COPY_CLEAR'),
  ('vacuum_reports', 'COPY_CLEAR'),
  ('backup_manifests', 'COPY_CLEAR'),
  ('integrity_runs', 'COPY_CLEAR');

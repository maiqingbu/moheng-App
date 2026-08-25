CREATE TABLE IF NOT EXISTS catalog_meta (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS catalog_migration_history (
  version INTEGER PRIMARY KEY NOT NULL,
  applied_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS projects (
  project_id TEXT PRIMARY KEY NOT NULL,
  project_path TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  lifecycle TEXT NOT NULL CHECK (lifecycle IN ('ACTIVE', 'ARCHIVED', 'DELETING')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_opened_at TEXT,
  project_schema_version TEXT NOT NULL,
  compatibility_state TEXT NOT NULL CHECK (compatibility_state IN ('COMPATIBLE', 'MIGRATION_REQUIRED', 'RECOVERY'))
);
CREATE TABLE IF NOT EXISTS catalog_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
);

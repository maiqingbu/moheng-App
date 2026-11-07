-- WM-1 phase 1 (WORK_MEMORY_DESIGN §3): the work's long-term memory of the
-- details its chapters fixed. Derived from each chapter's production
-- receipt and rebuilt from them whenever they change, so a project copy
-- starts it again empty.
CREATE TABLE IF NOT EXISTS work_memory (
  project_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  chapter_no INTEGER NOT NULL,
  subject TEXT NOT NULL,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  PRIMARY KEY (project_id, chapter_id, ordinal),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);
CREATE INDEX IF NOT EXISTS idx_work_memory_chapter_no ON work_memory(project_id, chapter_no);

-- Which receipt each chapter's memory was taken from.
CREATE TABLE IF NOT EXISTS work_memory_sources (
  project_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  receipt_hash TEXT NOT NULL,
  PRIMARY KEY (project_id, chapter_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy) VALUES
  ('work_memory', 'COPY_CLEAR'),
  ('work_memory_sources', 'COPY_CLEAR');

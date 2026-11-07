-- WM-2a (WORK_MEMORY_DESIGN §6.2, §8): the owner's corrections, the
-- highest-priority memory of a work. A writing rule holds for the whole
-- work (no subject); a remembered reminder holds for the person or thing it
-- names. Written by the owner, never derived, so they are kept on this
-- machine with the work and copied with it. A revoked one stays, marked.
CREATE TABLE IF NOT EXISTS owner_corrections (
  project_id TEXT NOT NULL,
  correction_id TEXT NOT NULL,
  subject TEXT,
  content TEXT NOT NULL CHECK (length(trim(content)) > 0),
  source_decision_id TEXT,
  created_at TEXT NOT NULL,
  revoked_at TEXT,
  PRIMARY KEY (project_id, correction_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy)
VALUES ('owner_corrections', 'COPY_REMAP');

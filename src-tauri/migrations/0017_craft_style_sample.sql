-- CRAFT v3 (CRAFT_V3_DESIGN §4.1): the user's style sample stays on this
-- machine. It is not an artifact, never enters a CommitBundle, export or
-- model prompt; style cards carry only its metrics and digest.
CREATE TABLE IF NOT EXISTS style_samples (
  project_id TEXT PRIMARY KEY,
  sample_digest TEXT NOT NULL CHECK (length(sample_digest) = 64),
  body TEXT NOT NULL,
  saved_at TEXT NOT NULL,
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

-- A project copy on this machine keeps its sample: copied style cards still
-- reference its digest.
INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy)
VALUES ('style_samples', 'COPY_REMAP');

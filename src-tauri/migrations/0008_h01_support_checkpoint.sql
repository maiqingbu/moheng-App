-- H01: durable operational checkpoint only. This table is not narrative
-- authority; it stores resumable read/diagnose session metadata.
CREATE TABLE IF NOT EXISTS support_checkpoints (
  checkpoint_id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  project_revision INTEGER NOT NULL CHECK (project_revision >= 0),
  question_digest TEXT NOT NULL CHECK (length(question_digest) = 64),
  plan_digest TEXT NOT NULL CHECK (length(plan_digest) = 64),
  completed_step_ids_json TEXT NOT NULL DEFAULT '[]',
  capability_spec_digests_json TEXT NOT NULL DEFAULT '{}',
  evidence_refs_json TEXT NOT NULL DEFAULT '[]',
  last_result_digests_json TEXT NOT NULL DEFAULT '[]',
  pending_authorization_ref TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_support_checkpoints_session
  ON support_checkpoints(project_id, session_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_obligation_evidence_project_obligation_created
ON obligation_evidence(project_id, obligation_id, created_at, evidence_id);

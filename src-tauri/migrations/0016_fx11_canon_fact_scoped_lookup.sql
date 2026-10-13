-- FX-11 / NFR-001: chapter- and predicate-scoped canon lookups must not walk
-- every fact in the project through (project_id, fact_id).
CREATE INDEX IF NOT EXISTS idx_canon_facts_valid_from_chapter
ON canon_facts(project_id, valid_from_chapter_id);
CREATE INDEX IF NOT EXISTS idx_canon_facts_predicate_subject
ON canon_facts(project_id, predicate, subject_id);
-- Ordered per-entity current-fact lookup: bounded by LIMIT even for an entity
-- that carries a very large number of facts.
CREATE INDEX IF NOT EXISTS idx_canon_facts_current_subject_fact
ON canon_facts(project_id, subject_id, fact_id) WHERE superseded_by IS NULL;

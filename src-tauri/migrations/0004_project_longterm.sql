-- M0B long-term authority & projection tables (V2.1 §21.2).
--
-- Applied as migration version 4 on top of the M0A authority layer (v2) and
-- the ProjectCommand layer (v3). It is written to be idempotent (CREATE
-- TABLE IF NOT EXISTS / INSERT OR IGNORE) so it can be replayed on a fresh
-- DB, on an existing M0A DB being upgraded, and on a Copy-Project remap.
--
-- Conventions:
--   * TEXT         — opaque stable IDs and RFC3339 UTC timestamps are stored
--                    as TEXT; business rowids are never exposed across the
--                    contract boundary (B01-T04).
--   * *_projection  — derived, rebuildable views of authority; never written
--                    as a source of truth.
--   * authority     — rows carry FK/CHECK/UNIQUE so the kernel can fail closed.
--   * project_id referenced everywhere so Copy-Project remap (B01-T08) can
--     rewrite a single column per table.

-- ---------------------------------------------------------------------------
-- 1. Project & state
-- ---------------------------------------------------------------------------

-- Runtime-state snapshot for a single project. This is NOT a second authority;
-- it is a derived, rebuildable summary the UI reads to avoid recomputing a
-- projection on every paint. The authoritative per-commit facts live in
-- commit_receipts + canonical_heads.
CREATE TABLE IF NOT EXISTS project_runtime_state (
  project_id TEXT PRIMARY KEY NOT NULL,
  current_phase TEXT NOT NULL DEFAULT 'SETUP',
  run_state TEXT NOT NULL DEFAULT 'IDLE',
  block_reason TEXT,
  signing_status TEXT NOT NULL DEFAULT 'NOT_STARTED',
  committed_chapters INTEGER NOT NULL DEFAULT 0 CHECK (committed_chapters >= 0),
  candidate_chapters INTEGER NOT NULL DEFAULT 0 CHECK (candidate_chapters >= 0),
  total_words INTEGER NOT NULL DEFAULT 0 CHECK (total_words >= 0),
  total_cost_cents INTEGER NOT NULL DEFAULT 0 CHECK (total_cost_cents >= 0),
  progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
  current_volume_id TEXT,
  current_chapter_no INTEGER,
  latest_receipt_id TEXT,
  generated_revision INTEGER NOT NULL DEFAULT 0 CHECK (generated_revision >= 0),
  updated_at TEXT NOT NULL,
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

-- User/project-level settings. Values are scalar/keyed so a setting can be
-- added without a migration. This table is authoritative for settings that
-- are owned by the project (as opposed to catalog-global settings).
CREATE TABLE IF NOT EXISTS project_settings (
  project_id TEXT NOT NULL,
  setting_key TEXT NOT NULL,
  setting_value TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, setting_key),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

-- Authoritative four-layer hierarchy: Book -> Volume -> Arc -> Chapter.
-- Stable opaque IDs (B01-T04) identify each row; title/number are display
-- fields only and never act as a business key.
CREATE TABLE IF NOT EXISTS books (
  book_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  book_no INTEGER NOT NULL CHECK (book_no > 0),
  title TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'PLANNING' CHECK (status IN ('PLANNING','ACTIVE','COMPLETED','ARCHIVED')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (book_id),
  UNIQUE (project_id, book_no),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS volumes (
  volume_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  book_id TEXT NOT NULL,
  volume_no INTEGER NOT NULL CHECK (volume_no > 0),
  title TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'PLANNING' CHECK (status IN ('PLANNING','READY','PRODUCING','PAUSED','REVIEW','CLOSED','ARCHIVED')),
  chapter_start INTEGER NOT NULL DEFAULT 0 CHECK (chapter_start >= 0),
  chapter_end INTEGER NOT NULL DEFAULT 0 CHECK (chapter_end >= 0),
  target_words INTEGER NOT NULL DEFAULT 0 CHECK (target_words >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (volume_id),
  UNIQUE (project_id, book_id, volume_no),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (book_id) REFERENCES books(book_id),
  CHECK (chapter_end >= chapter_start)
);

CREATE TABLE IF NOT EXISTS arcs (
  arc_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  arc_no INTEGER NOT NULL CHECK (arc_no > 0),
  title TEXT NOT NULL,
  chapter_start INTEGER NOT NULL DEFAULT 0 CHECK (chapter_start >= 0),
  chapter_end INTEGER NOT NULL DEFAULT 0 CHECK (chapter_end >= 0),
  status TEXT NOT NULL DEFAULT 'PLANNING' CHECK (status IN ('PLANNING','ACTIVE','DONE','ABANDONED')),
  goal TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (arc_id),
  UNIQUE (project_id, volume_id, arc_no),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id),
  CHECK (chapter_end >= chapter_start)
);

CREATE TABLE IF NOT EXISTS chapters (
  chapter_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  arc_id TEXT,
  chapter_no INTEGER NOT NULL CHECK (chapter_no > 0),
  title TEXT NOT NULL,
  importance_class TEXT NOT NULL DEFAULT 'NORMAL' CHECK (importance_class IN ('NORMAL','IMPORTANT','CRITICAL')),
  status TEXT NOT NULL DEFAULT 'PLANNED' CHECK (status IN ('PLANNED','PRODUCING','COMMITTED','PAUSED','REJECTED','SUPERSEDED')),
  word_count INTEGER NOT NULL DEFAULT 0 CHECK (word_count >= 0),
  final_artifact_id TEXT,
  final_version INTEGER,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (chapter_id),
  UNIQUE (project_id, volume_id, chapter_no),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id),
  FOREIGN KEY (arc_id) REFERENCES arcs(arc_id)
);

-- ---------------------------------------------------------------------------
-- 2. Artifact (extends the M0A layer)
-- ---------------------------------------------------------------------------

-- Frozen ArtifactType-specific schema descriptors. This is a validation
-- registry, not an authority table; it records which JSON Schema a given
-- artifact_type conforms to so the kernel can fail closed on unknown shapes.
CREATE TABLE IF NOT EXISTS artifact_schemas (
  project_id TEXT NOT NULL,
  artifact_type TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  schema_json TEXT NOT NULL,
  frozen_at TEXT NOT NULL,
  PRIMARY KEY (project_id, artifact_type),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

-- ---------------------------------------------------------------------------
-- 3. Long-form planning index
-- ---------------------------------------------------------------------------

-- Each *_plan_index is a derived, rebuildable projection of the current
-- APPROVED_PLAN artifact for its scope. These are never a second authority:
-- the authoritative plan is the committed Artifact. On rebuild they are
-- regenerated from the artifact payload.
CREATE TABLE IF NOT EXISTS book_plan_index (
  project_id TEXT NOT NULL,
  book_id TEXT NOT NULL,
  plan_artifact_id TEXT NOT NULL,
  plan_version INTEGER NOT NULL,
  plan_state TEXT NOT NULL,
  target_volumes INTEGER NOT NULL DEFAULT 0 CHECK (target_volumes >= 0),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, book_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (book_id) REFERENCES books(book_id)
);

CREATE TABLE IF NOT EXISTS volume_plan_index (
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  plan_artifact_id TEXT NOT NULL,
  plan_version INTEGER NOT NULL,
  plan_state TEXT NOT NULL,
  target_chapters INTEGER NOT NULL DEFAULT 0 CHECK (target_chapters >= 0),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS arc_plan_index (
  project_id TEXT NOT NULL,
  arc_id TEXT NOT NULL,
  plan_artifact_id TEXT NOT NULL,
  plan_version INTEGER NOT NULL,
  plan_state TEXT NOT NULL,
  chapter_span INTEGER NOT NULL DEFAULT 0 CHECK (chapter_span >= 0),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, arc_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (arc_id) REFERENCES arcs(arc_id)
);

CREATE TABLE IF NOT EXISTS chapter_plan_index (
  project_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  plan_artifact_id TEXT NOT NULL,
  plan_version INTEGER NOT NULL,
  plan_state TEXT NOT NULL,
  task_input_scope TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, chapter_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (chapter_id) REFERENCES chapters(chapter_id)
);

-- Per-volume planning ledger: entry state, obligation/foreshadow intents,
-- planned materialization positions, evidence needs, and repair status.
CREATE TABLE IF NOT EXISTS planning_state_ledger_index (
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  entry_state TEXT NOT NULL,
  plan_state TEXT NOT NULL,
  coverage_state TEXT NOT NULL DEFAULT 'INCOMPLETE',
  obligation_ids_json TEXT NOT NULL DEFAULT '[]',
  foreshadow_ids_json TEXT NOT NULL DEFAULT '[]',
  evidence_need_ids_json TEXT NOT NULL DEFAULT '[]',
  repair_state TEXT NOT NULL DEFAULT 'NONE',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

-- ---------------------------------------------------------------------------
-- 4. Canon (normalized, not one big JSON — §11)
-- ---------------------------------------------------------------------------

-- Canon entities carry a stable opaque id; name is a display/alias field only.
CREATE TABLE IF NOT EXISTS canon_entities (
  entity_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  canonical_name TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  first_chapter_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (entity_id),
  UNIQUE (project_id, entity_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS canon_aliases (
  alias_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  alias TEXT NOT NULL,
  alias_type TEXT NOT NULL DEFAULT 'NAME',
  PRIMARY KEY (alias_id),
  UNIQUE (project_id, entity_id, alias),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (entity_id) REFERENCES canon_entities(entity_id)
);

CREATE TABLE IF NOT EXISTS canon_facts (
  fact_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  predicate TEXT NOT NULL,
  object_json TEXT NOT NULL,
  valid_from_chapter_id TEXT,
  valid_to_chapter_id TEXT,
  story_time_from TEXT,
  story_time_to TEXT,
  source_artifact_id TEXT,
  source_version INTEGER,
  evidence_range_json TEXT,
  authority_level TEXT NOT NULL DEFAULT 'C' CHECK (authority_level IN ('A','B','C','D')),
  superseded_by TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (fact_id),
  UNIQUE (project_id, fact_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (subject_id) REFERENCES canon_entities(entity_id)
);

CREATE TABLE IF NOT EXISTS canon_relations (
  relation_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  predicate TEXT NOT NULL,
  object_id TEXT NOT NULL,
  valid_from_chapter_id TEXT,
  valid_to_chapter_id TEXT,
  story_time_from TEXT,
  story_time_to TEXT,
  source_artifact_id TEXT,
  source_version INTEGER,
  authority_level TEXT NOT NULL DEFAULT 'C' CHECK (authority_level IN ('A','B','C','D')),
  superseded_by TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (relation_id),
  UNIQUE (project_id, relation_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (subject_id) REFERENCES canon_entities(entity_id),
  FOREIGN KEY (object_id) REFERENCES canon_entities(entity_id)
);

CREATE TABLE IF NOT EXISTS canon_knowledge (
  knowledge_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  knower_id TEXT NOT NULL,
  fact_id TEXT,
  epistemic_state TEXT NOT NULL CHECK (epistemic_state IN ('KNOWS','BELIEVES_TRUE','BELIEVES_FALSE','SUSPECTS','CONCEALS','UNKNOWN')),
  visibility_scope TEXT,
  from_chapter_id TEXT,
  until_chapter_id TEXT,
  source_artifact_id TEXT,
  source_version INTEGER,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (knowledge_id),
  UNIQUE (project_id, knowledge_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (knower_id) REFERENCES canon_entities(entity_id)
);

CREATE TABLE IF NOT EXISTS canon_events (
  event_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  subject_id TEXT,
  object_json TEXT,
  story_time TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  PRIMARY KEY (event_id),
  UNIQUE (project_id, event_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (chapter_id) REFERENCES chapters(chapter_id)
);

CREATE TABLE IF NOT EXISTS canon_evidence (
  evidence_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  fact_id TEXT,
  relation_id TEXT,
  event_id TEXT,
  final_chapter_id TEXT NOT NULL,
  source_artifact_id TEXT NOT NULL,
  source_version INTEGER NOT NULL,
  evidence_type TEXT NOT NULL,
  quote TEXT,
  range_start INTEGER,
  range_end INTEGER,
  created_at TEXT NOT NULL,
  PRIMARY KEY (evidence_id),
  UNIQUE (project_id, evidence_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (final_chapter_id) REFERENCES chapters(chapter_id),
  FOREIGN KEY (source_artifact_id, source_version) REFERENCES artifacts(artifact_id, version)
);

-- Canon time points normalize story-time ordering (§11.2). Not in the §21.2
-- list of 61 but required by the Canon data model (§11.1) for time validity.
CREATE TABLE IF NOT EXISTS canon_timepoints (
  timepoint_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  story_time TEXT,
  ordering INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (timepoint_id),
  UNIQUE (project_id, chapter_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (chapter_id) REFERENCES chapters(chapter_id)
);

-- ---------------------------------------------------------------------------
-- 5. Long-term obligations, foreshadow, reader desire (B01-T01)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS narrative_obligations (
  obligation_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  scope TEXT NOT NULL CHECK (scope IN ('BOOK','VOLUME','ARC','CHAPTER')),
  obligation_type TEXT NOT NULL,
  authority_level TEXT NOT NULL CHECK (authority_level IN ('A','B','C')),
  source_artifact_id TEXT NOT NULL,
  source_version INTEGER NOT NULL,
  description TEXT NOT NULL,
  must_start_before_chapter TEXT,
  must_resolve_before_chapter TEXT,
  forbidden_before_chapter TEXT,
  prerequisites_json TEXT NOT NULL DEFAULT '[]',
  lifecycle TEXT NOT NULL DEFAULT 'PROPOSED' CHECK (lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING','RESOLVED','ABANDONED','SUPERSEDED')),
  last_touched_chapter_id TEXT,
  aging_score REAL NOT NULL DEFAULT 0 CHECK (aging_score >= 0),
  decision_ref TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (obligation_id),
  UNIQUE (project_id, obligation_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (source_artifact_id, source_version) REFERENCES artifacts(artifact_id, version)
);

CREATE TABLE IF NOT EXISTS obligation_dependencies (
  obligation_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  depends_on_obligation_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (obligation_id, depends_on_obligation_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (obligation_id) REFERENCES narrative_obligations(obligation_id),
  FOREIGN KEY (depends_on_obligation_id) REFERENCES narrative_obligations(obligation_id)
);

CREATE TABLE IF NOT EXISTS obligation_evidence (
  evidence_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  obligation_id TEXT NOT NULL,
  evidence_type TEXT NOT NULL CHECK (evidence_type IN ('COMPLETION','PROGRESS','ABANDON')),
  artifact_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (evidence_id),
  UNIQUE (project_id, evidence_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (obligation_id) REFERENCES narrative_obligations(obligation_id),
  FOREIGN KEY (artifact_id, version) REFERENCES artifacts(artifact_id, version)
);

CREATE TABLE IF NOT EXISTS foreshadows (
  foreshadow_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  intent TEXT NOT NULL,
  secret_payload_ref TEXT,
  plant_chapter_id TEXT,
  reinforce_chapters_json TEXT NOT NULL DEFAULT '[]',
  recover_window_end_chapter TEXT,
  forbidden_reveal_before_chapter TEXT,
  planting_evidence_json TEXT NOT NULL DEFAULT '[]',
  recovery_evidence_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'PLANTED' CHECK (status IN ('PLANTED','REINFORCED','RECOVERED','ABANDONED','STALE')),
  source_authority TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (foreshadow_id),
  UNIQUE (project_id, foreshadow_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS foreshadow_windows (
  window_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  foreshadow_id TEXT NOT NULL,
  window_type TEXT NOT NULL CHECK (window_type IN ('PLANT','REINFORCE','RECOVER')),
  chapter_start TEXT,
  chapter_end TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (window_id),
  UNIQUE (project_id, window_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (foreshadow_id) REFERENCES foreshadows(foreshadow_id)
);

CREATE TABLE IF NOT EXISTS character_arc_states (
  arc_state_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  character_id TEXT NOT NULL,
  arc_id TEXT NOT NULL,
  arc_type TEXT NOT NULL,
  state TEXT NOT NULL,
  last_touched_chapter_id TEXT,
  aging_score REAL NOT NULL DEFAULT 0 CHECK (aging_score >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (arc_state_id),
  UNIQUE (project_id, character_id, arc_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (character_id) REFERENCES canon_entities(entity_id),
  FOREIGN KEY (arc_id) REFERENCES arcs(arc_id)
);

CREATE TABLE IF NOT EXISTS mystery_states (
  mystery_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  mystery_type TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN','PROGRESSING','RESOLVED','ABANDONED')),
  core_fact_id TEXT,
  reveal_before_chapter TEXT,
  aging_score REAL NOT NULL DEFAULT 0 CHECK (aging_score >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (mystery_id),
  UNIQUE (project_id, mystery_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS reader_desires (
  desire_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  desire_type TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','PAID','ABANDONED','STALE')),
  expected_payoff_chapter TEXT,
  created_chapter_id TEXT,
  last_touched_chapter_id TEXT,
  aging_score REAL NOT NULL DEFAULT 0 CHECK (aging_score >= 0),
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (desire_id),
  UNIQUE (project_id, desire_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS reader_desire_events (
  desire_event_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  desire_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  PRIMARY KEY (desire_event_id),
  UNIQUE (project_id, desire_event_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (desire_id) REFERENCES reader_desires(desire_id),
  FOREIGN KEY (chapter_id) REFERENCES chapters(chapter_id)
);

CREATE TABLE IF NOT EXISTS reader_desire_evidence (
  evidence_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  desire_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  evidence_type TEXT NOT NULL,
  artifact_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (evidence_id),
  UNIQUE (project_id, evidence_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (desire_id) REFERENCES reader_desires(desire_id),
  FOREIGN KEY (chapter_id) REFERENCES chapters(chapter_id),
  FOREIGN KEY (artifact_id, version) REFERENCES artifacts(artifact_id, version)
);

-- Derived/current taste-profile lookup. The authoritative profile is the
-- NarrativeTasteProfile Artifact; this is a rebuildable index.
CREATE TABLE IF NOT EXISTS narrative_taste_profile_index (
  project_id TEXT NOT NULL,
  profile_artifact_id TEXT NOT NULL,
  profile_version INTEGER NOT NULL,
  genre TEXT,
  profile_label TEXT,
  quality_weights_json TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

-- ---------------------------------------------------------------------------
-- 6. Typed dependency / change / retcon (B01-T01)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS dependency_edges (
  edge_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  target_ref TEXT NOT NULL,
  dependency_type TEXT NOT NULL CHECK (dependency_type IN ('HARD_CONTRACT','PLAN_INPUT','CANON_FACT','STATE_INPUT','KNOWLEDGE_INPUT','FORESHADOW_INPUT','OBLIGATION_INPUT','EVIDENCE','PRESENTATION','SOFT_CONTEXT')),
  scope TEXT NOT NULL,
  impact_class TEXT NOT NULL CHECK (impact_class IN ('HARD','REVIEW_REQUIRED','SOFT','EVIDENCE')),
  source_hash TEXT NOT NULL CHECK (length(source_hash) = 64),
  selector TEXT,
  entity_id TEXT,
  chapter_range_json TEXT,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (edge_id),
  UNIQUE (project_id, edge_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS change_requests (
  change_request_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  change_type TEXT NOT NULL,
  scope TEXT NOT NULL,
  summary TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'DRAFT' CHECK (status IN ('DRAFT','IMPACT_READY','PENDING_APPROVAL','APPROVED','APPLYING','APPLIED','REJECTED')),
  impact_seed_json TEXT NOT NULL DEFAULT '{}',
  decision_ref TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (change_request_id),
  UNIQUE (project_id, change_request_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS impact_sets (
  impact_set_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  change_request_id TEXT NOT NULL,
  scope_class TEXT NOT NULL CHECK (scope_class IN ('LOCAL','WIDE','GLOBAL')),
  impact_estimate_json TEXT NOT NULL DEFAULT '{}',
  preflight_state TEXT NOT NULL DEFAULT 'PLANNING',
  processed_pages_json TEXT NOT NULL DEFAULT '[]',
  total_edges INTEGER NOT NULL DEFAULT 0 CHECK (total_edges >= 0),
  expanded_edges INTEGER NOT NULL DEFAULT 0 CHECK (expanded_edges >= 0),
  checkpoint_offset INTEGER NOT NULL DEFAULT 0 CHECK (checkpoint_offset >= 0),
  status TEXT NOT NULL DEFAULT 'PLANNING' CHECK (status IN ('PLANNING','RUNNING','PAUSED','CANCELLED','COMPLETED','FAILED')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (impact_set_id),
  UNIQUE (project_id, impact_set_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (change_request_id) REFERENCES change_requests(change_request_id)
);

CREATE TABLE IF NOT EXISTS retcon_plans (
  retcon_plan_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  target_fact_json TEXT NOT NULL DEFAULT '{}',
  reason TEXT NOT NULL,
  fix_type TEXT NOT NULL CHECK (fix_type IN ('EXPLANATORY_PATCH','REWRITE','SUPPLEMENT')),
  downstream_impact_json TEXT NOT NULL DEFAULT '{}',
  requires_resigning INTEGER NOT NULL DEFAULT 0 CHECK (requires_resigning IN (0, 1)),
  estimated_cost_cents INTEGER NOT NULL DEFAULT 0 CHECK (estimated_cost_cents >= 0),
  approved_by TEXT,
  status TEXT NOT NULL DEFAULT 'DRAFT' CHECK (status IN ('DRAFT','PENDING_APPROVAL','APPROVED','APPLIED','REJECTED')),
  decision_ref TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (retcon_plan_id),
  UNIQUE (project_id, retcon_plan_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS stale_queue (
  stale_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  change_request_id TEXT,
  target_ref TEXT NOT NULL,
  stale_reason TEXT NOT NULL,
  revalidate_class TEXT NOT NULL CHECK (revalidate_class IN ('REVALIDATE_ONLY','REPLAN','REWRITE','RETCON')),
  processed INTEGER NOT NULL DEFAULT 0 CHECK (processed IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (stale_id),
  UNIQUE (project_id, stale_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (change_request_id) REFERENCES change_requests(change_request_id)
);

-- ---------------------------------------------------------------------------
-- 7. Production runtime (B01-T01)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS jobs (
  job_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  job_type TEXT NOT NULL CHECK (job_type IN ('BOOK','VOLUME','CHAPTER')),
  book_id TEXT,
  volume_id TEXT,
  chapter_id TEXT,
  status TEXT NOT NULL DEFAULT 'QUEUED' CHECK (status IN ('QUEUED','RUNNING','PAUSING','PAUSED','WAITING_MODEL','WAITING_DECISION','WAITING_BUDGET','BLOCKED','COMPLETED','FAILED','CANCELLED')),
  current_stage TEXT,
  attempt_no INTEGER NOT NULL DEFAULT 0 CHECK (attempt_no >= 0),
  repair_round INTEGER NOT NULL DEFAULT 0 CHECK (repair_round >= 0),
  error_code TEXT,
  parent_job_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (job_id),
  UNIQUE (project_id, job_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (parent_job_id) REFERENCES jobs(job_id)
);

CREATE TABLE IF NOT EXISTS job_steps (
  job_step_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  job_id TEXT NOT NULL,
  step_no INTEGER NOT NULL CHECK (step_no > 0),
  step_type TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'QUEUED' CHECK (status IN ('QUEUED','RUNNING','WAITING_MODEL','COMPLETED','FAILED','CANCELLED','SKIPPED')),
  model_run_id TEXT,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (job_step_id),
  UNIQUE (project_id, job_step_id),
  UNIQUE (project_id, job_id, step_no),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (job_id) REFERENCES jobs(job_id)
);

CREATE TABLE IF NOT EXISTS job_leases (
  lease_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  job_id TEXT NOT NULL,
  holder TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  acquired_at TEXT NOT NULL,
  released_at TEXT,
  PRIMARY KEY (lease_id),
  UNIQUE (project_id, job_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (job_id) REFERENCES jobs(job_id)
);

CREATE TABLE IF NOT EXISTS job_checkpoints (
  checkpoint_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  job_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  offset INTEGER NOT NULL DEFAULT 0 CHECK (offset >= 0),
  boundary_json TEXT NOT NULL DEFAULT '{}',
  saved_at TEXT NOT NULL,
  PRIMARY KEY (checkpoint_id),
  UNIQUE (project_id, job_id, stage),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (job_id) REFERENCES jobs(job_id)
);

CREATE TABLE IF NOT EXISTS production_events (
  event_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  job_id TEXT,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  PRIMARY KEY (event_id),
  UNIQUE (project_id, event_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (job_id) REFERENCES jobs(job_id)
);

-- ---------------------------------------------------------------------------
-- 8. Model / budget (B01-T01)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS model_runs (
  model_run_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  step_id TEXT,
  job_id TEXT,
  prompt_ref TEXT,
  context_manifest_ref TEXT,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  capability TEXT NOT NULL,
  attempt_no INTEGER NOT NULL DEFAULT 0 CHECK (attempt_no >= 0),
  repair_round INTEGER NOT NULL DEFAULT 0 CHECK (repair_round >= 0),
  predicted_cost_cents INTEGER NOT NULL DEFAULT 0 CHECK (predicted_cost_cents >= 0),
  reserved_cost_cents INTEGER NOT NULL DEFAULT 0 CHECK (reserved_cost_cents >= 0),
  budget_reservation_id TEXT,
  status TEXT NOT NULL DEFAULT 'PREPARED' CHECK (status IN ('PREPARED','RUNNING','UNKNOWN_REMOTE_OUTCOME','COMPLETED','FAILED','CANCELLED','REPLAYED')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (model_run_id),
  UNIQUE (project_id, model_run_id),
  UNIQUE (project_id, idempotency_key),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (job_id) REFERENCES jobs(job_id),
  FOREIGN KEY (step_id) REFERENCES job_steps(job_step_id)
);

CREATE TABLE IF NOT EXISTS model_responses (
  response_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  model_run_id TEXT NOT NULL,
  raw_response TEXT NOT NULL,
  response_digest TEXT NOT NULL CHECK (length(response_digest) = 64),
  received_at TEXT NOT NULL,
  PRIMARY KEY (response_id),
  UNIQUE (project_id, response_id),
  UNIQUE (project_id, model_run_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (model_run_id) REFERENCES model_runs(model_run_id)
);

CREATE TABLE IF NOT EXISTS model_parse_results (
  parse_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  model_run_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('OK','SCHEMA_INVALID','EMPTY','PARTIAL')),
  parsed_json TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (parse_id),
  UNIQUE (project_id, parse_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (model_run_id) REFERENCES model_runs(model_run_id)
);

CREATE TABLE IF NOT EXISTS model_usage (
  usage_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  model_run_id TEXT NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
  latency_ms INTEGER NOT NULL DEFAULT 0 CHECK (latency_ms >= 0),
  cost_cents INTEGER NOT NULL DEFAULT 0 CHECK (cost_cents >= 0),
  recorded_at TEXT NOT NULL,
  PRIMARY KEY (usage_id),
  UNIQUE (project_id, usage_id),
  UNIQUE (project_id, model_run_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (model_run_id) REFERENCES model_runs(model_run_id)
);

CREATE TABLE IF NOT EXISTS budget_reservations (
  reservation_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  model_run_id TEXT,
  budget_ref TEXT NOT NULL,
  reserved_cents INTEGER NOT NULL DEFAULT 0 CHECK (reserved_cents >= 0),
  settled_cents INTEGER NOT NULL DEFAULT 0 CHECK (settled_cents >= 0),
  status TEXT NOT NULL DEFAULT 'RESERVED' CHECK (status IN ('RESERVED','SETTLED','RELEASED','OVER_LIMIT')),
  created_at TEXT NOT NULL,
  settled_at TEXT,
  PRIMARY KEY (reservation_id),
  UNIQUE (project_id, reservation_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (model_run_id) REFERENCES model_runs(model_run_id)
);

CREATE TABLE IF NOT EXISTS prompt_registry (
  prompt_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  prompt_type TEXT NOT NULL,
  provider TEXT,
  model TEXT,
  prompt_text TEXT NOT NULL,
  prompt_digest TEXT NOT NULL CHECK (length(prompt_digest) = 64),
  version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
  created_at TEXT NOT NULL,
  PRIMARY KEY (prompt_id),
  UNIQUE (project_id, prompt_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS chapter_production_budgets (
  budget_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  importance_class TEXT NOT NULL CHECK (importance_class IN ('NORMAL','IMPORTANT','CRITICAL')),
  model_policy_ref TEXT,
  target_model_runs INTEGER NOT NULL DEFAULT 0 CHECK (target_model_runs >= 0),
  soft_max_model_runs INTEGER NOT NULL DEFAULT 0 CHECK (soft_max_model_runs >= 0),
  hard_max_model_runs INTEGER NOT NULL DEFAULT 0 CHECK (hard_max_model_runs >= 0),
  input_token_budget INTEGER NOT NULL DEFAULT 0 CHECK (input_token_budget >= 0),
  output_token_budget INTEGER NOT NULL DEFAULT 0 CHECK (output_token_budget >= 0),
  repair_run_limit INTEGER NOT NULL DEFAULT 0 CHECK (repair_run_limit >= 0),
  wall_clock_budget_ms INTEGER NOT NULL DEFAULT 0 CHECK (wall_clock_budget_ms >= 0),
  cost_budget_cents INTEGER NOT NULL DEFAULT 0 CHECK (cost_budget_cents >= 0),
  strategy_search_budget INTEGER NOT NULL DEFAULT 0 CHECK (strategy_search_budget >= 0),
  frozen_at TEXT NOT NULL,
  PRIMARY KEY (budget_id),
  UNIQUE (project_id, chapter_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (chapter_id) REFERENCES chapters(chapter_id)
);

CREATE TABLE IF NOT EXISTS model_cost_ledger (
  ledger_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  model_run_id TEXT NOT NULL,
  budget_id TEXT,
  cost_cents INTEGER NOT NULL DEFAULT 0 CHECK (cost_cents >= 0),
  token_total INTEGER NOT NULL DEFAULT 0 CHECK (token_total >= 0),
  cost_type TEXT NOT NULL CHECK (cost_type IN ('RESERVED','ACTUAL','REFUND','OVERAGE')),
  created_at TEXT NOT NULL,
  PRIMARY KEY (ledger_id),
  UNIQUE (project_id, ledger_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (model_run_id) REFERENCES model_runs(model_run_id)
);

-- ---------------------------------------------------------------------------
-- 9. Search / projection (derived, rebuildable — §22)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS chapter_index (
  project_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  title TEXT NOT NULL,
  chapter_no INTEGER NOT NULL CHECK (chapter_no > 0),
  volume_id TEXT NOT NULL,
  committed INTEGER NOT NULL DEFAULT 0 CHECK (committed IN (0, 1)),
  word_count INTEGER NOT NULL DEFAULT 0 CHECK (word_count >= 0),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, chapter_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (chapter_id) REFERENCES chapters(chapter_id)
);

CREATE TABLE IF NOT EXISTS entity_index (
  project_id TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  canonical_name TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  PRIMARY KEY (project_id, entity_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (entity_id) REFERENCES canon_entities(entity_id)
);

CREATE TABLE IF NOT EXISTS volume_health_projection (
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  score REAL NOT NULL DEFAULT 0 CHECK (score >= 0 AND score <= 1),
  level TEXT NOT NULL DEFAULT 'healthy' CHECK (level IN ('healthy','attention','risk')),
  open_obligations INTEGER NOT NULL DEFAULT 0 CHECK (open_obligations >= 0),
  overdue_foreshadows INTEGER NOT NULL DEFAULT 0 CHECK (overdue_foreshadows >= 0),
  checked_at TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  PRIMARY KEY (project_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS book_health_projection (
  project_id TEXT NOT NULL,
  book_id TEXT NOT NULL,
  score REAL NOT NULL DEFAULT 0 CHECK (score >= 0 AND score <= 1),
  level TEXT NOT NULL DEFAULT 'healthy' CHECK (level IN ('healthy','attention','risk')),
  open_obligations INTEGER NOT NULL DEFAULT 0 CHECK (open_obligations >= 0),
  progress_to_ending REAL NOT NULL DEFAULT 0 CHECK (progress_to_ending >= 0 AND progress_to_ending <= 1),
  checked_at TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  PRIMARY KEY (project_id, book_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (book_id) REFERENCES books(book_id)
);

CREATE TABLE IF NOT EXISTS production_timeline_projection (
  project_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  job_id TEXT,
  event_type TEXT NOT NULL,
  event_order INTEGER NOT NULL DEFAULT 0 CHECK (event_order >= 0),
  happened_at TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  PRIMARY KEY (project_id, event_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS reader_desire_health_projection (
  project_id TEXT NOT NULL,
  checked_at TEXT NOT NULL,
  open_desires INTEGER NOT NULL DEFAULT 0 CHECK (open_desires >= 0),
  overdue_desires INTEGER NOT NULL DEFAULT 0 CHECK (overdue_desires >= 0),
  paid_desires INTEGER NOT NULL DEFAULT 0 CHECK (paid_desires >= 0),
  abandoned_desires INTEGER NOT NULL DEFAULT 0 CHECK (abandoned_desires >= 0),
  payload_json TEXT NOT NULL DEFAULT '{}',
  PRIMARY KEY (project_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS quality_trend_projection (
  project_id TEXT NOT NULL,
  sample_chapter_id TEXT NOT NULL,
  quality_vector_json TEXT NOT NULL DEFAULT '{}',
  sample_chapter_no INTEGER NOT NULL DEFAULT 0 CHECK (sample_chapter_no >= 0),
  sampled_at TEXT NOT NULL,
  PRIMARY KEY (project_id, sample_chapter_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (sample_chapter_id) REFERENCES chapters(chapter_id)
);

-- ---------------------------------------------------------------------------
-- 10. Authority registry alignment (B01-T03)
-- ---------------------------------------------------------------------------
-- The M0A layer reserved `reader_desire_ledger` and `narrative_quality_reports`
-- as forward-declared names but never created the tables, and their names did
-- not match the frozen V2.1 model. Correct them to the now-real frozen tables
-- and register every long-term authority/projection the migration creates, so
-- no registered authoritative_table/table_name points at a missing table.
--
-- Re-application must stay idempotent: delete the table_family references that
-- KEY on the old keys *before* deleting the authority keys, so a re-run never
-- orphans a child row against the authority_registry FK.

DELETE FROM table_family_registry
  WHERE authority_key IN ('reader_desire', 'quality_vector');
DELETE FROM table_family_registry
  WHERE table_name IN ('reader_desire_ledger', 'narrative_quality_reports');
DELETE FROM authority_registry
  WHERE authority_key IN ('reader_desire', 'quality_vector');

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only) VALUES
  -- Project & state
  ('project_runtime_state', 'project_runtime_state', 'ProjectManager', 1),
  ('project_settings', 'project_settings', 'ProjectManager', 0),
  ('book', 'books', 'M1', 0),
  ('volume', 'volumes', 'M3A', 0),
  ('arc', 'arcs', 'M3A', 0),
  ('chapter', 'chapters', 'M3A', 0),
  -- Artifact schema registry (authoritative for which JSON Schema a type conforms to)
  ('artifact_schemas_registry', 'artifact_schemas', 'M3A', 0),
  -- Planning index (derived)
  ('book_plan_index', 'book_plan_index', 'M1', 1),
  ('volume_plan_index', 'volume_plan_index', 'M3A', 1),
  ('arc_plan_index', 'arc_plan_index', 'M3A', 1),
  ('chapter_plan_index', 'chapter_plan_index', 'M3A', 1),
  ('planning_state_ledger', 'planning_state_ledger_index', 'M3A', 1),
  -- Canon (authoritative)
  ('canon_entity', 'canon_entities', 'M3B', 0),
  ('canon_alias', 'canon_aliases', 'M3B', 0),
  ('canon_fact', 'canon_facts', 'M3B', 0),
  ('canon_relation', 'canon_relations', 'M3B', 0),
  ('canon_knowledge', 'canon_knowledge', 'M3B', 0),
  ('canon_event', 'canon_events', 'M3B', 0),
  ('canon_evidence', 'canon_evidence', 'M3B', 0),
  ('canon_timepoint', 'canon_timepoints', 'M3B', 0),
  -- Long-term narrative control (authoritative)
  ('obligation', 'narrative_obligations', 'M3B', 0),
  ('obligation_dependency', 'obligation_dependencies', 'M3B', 0),
  ('obligation_evidence', 'obligation_evidence', 'M3B', 0),
  ('foreshadow', 'foreshadows', 'M3B', 0),
  ('foreshadow_window', 'foreshadow_windows', 'M3B', 0),
  ('character_arc_state', 'character_arc_states', 'M3B', 0),
  ('mystery_state', 'mystery_states', 'M3B', 0),
  ('reader_desire', 'reader_desires', 'M3B', 0),
  ('reader_desire_event', 'reader_desire_events', 'M3B', 0),
  ('reader_desire_evidence', 'reader_desire_evidence', 'M3B', 0),
  ('narrative_taste_profile', 'narrative_taste_profile_index', 'M3C', 1),
  -- Typed dependency / change / retcon (authoritative)
  ('dependency_edge', 'dependency_edges', 'M4A', 0),
  ('change_request', 'change_requests', 'M4A', 0),
  ('impact_set', 'impact_sets', 'M4A', 0),
  ('retcon_plan', 'retcon_plans', 'M4A', 0),
  ('stale_queue', 'stale_queue', 'M4A', 0),
  -- Production runtime (authoritative)
  ('job', 'jobs', 'ProductionSupervisor', 0),
  ('job_step', 'job_steps', 'ProductionSupervisor', 0),
  ('job_lease', 'job_leases', 'ProductionSupervisor', 0),
  ('job_checkpoint', 'job_checkpoints', 'ProductionSupervisor', 0),
  ('production_event', 'production_events', 'ProductionSupervisor', 0),
  -- Model & budget (authoritative)
  ('model_run', 'model_runs', 'ModelTransport', 0),
  ('model_response', 'model_responses', 'ModelTransport', 0),
  ('model_parse_result', 'model_parse_results', 'ModelTransport', 0),
  ('model_usage', 'model_usage', 'BudgetService', 0),
  ('budget_reservation', 'budget_reservations', 'BudgetService', 0),
  ('prompt_registry', 'prompt_registry', 'ProductionSupervisor', 0),
  ('quality_vector', 'quality_trend_projection', 'M3C', 1),
  ('model_cost_ledger', 'model_cost_ledger', 'BudgetService', 0),
  -- Search / projection (derived)
  ('chapter_index', 'chapter_index', 'SearchService', 1),
  ('entity_index', 'entity_index', 'SearchService', 1),
  ('volume_health_projection', 'volume_health_projection', 'M3C', 1),
  ('book_health_projection', 'book_health_projection', 'M3C', 1),
  ('production_timeline_projection', 'production_timeline_projection', 'ProductionSupervisor', 1),
  ('reader_desire_health_projection', 'reader_desire_health_projection', 'M3C', 1),
  ('production_budget', 'chapter_production_budgets', 'M3B', 0);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class) VALUES
  -- Project & state
  ('project_runtime_state', 'project', 'project_runtime_state', 'PERMANENT'),
  ('project_settings', 'project', 'project_settings', 'PERMANENT'),
  ('books', 'structural', 'book', 'PERMANENT'),
  ('volumes', 'structural', 'volume', 'PERMANENT'),
  ('arcs', 'structural', 'arc', 'PERMANENT'),
  ('chapters', 'structural', 'chapter', 'PERMANENT'),
  -- Planning index (derived)
  ('book_plan_index', 'planning_projection', 'book_plan_index', 'POLICY'),
  ('volume_plan_index', 'planning_projection', 'volume_plan_index', 'POLICY'),
  ('arc_plan_index', 'planning_projection', 'arc_plan_index', 'POLICY'),
  ('chapter_plan_index', 'planning_projection', 'chapter_plan_index', 'POLICY'),
  ('planning_state_ledger_index', 'planning_projection', 'planning_state_ledger', 'POLICY'),
  -- Canon
  ('canon_entities', 'canon', 'canon_entity', 'PERMANENT'),
  ('canon_aliases', 'canon', 'canon_alias', 'PERMANENT'),
  ('canon_facts', 'canon', 'canon_fact', 'PERMANENT'),
  ('canon_relations', 'canon', 'canon_relation', 'PERMANENT'),
  ('canon_knowledge', 'canon', 'canon_knowledge', 'PERMANENT'),
  ('canon_events', 'canon', 'canon_event', 'PERMANENT'),
  ('canon_evidence', 'canon', 'canon_evidence', 'PERMANENT'),
  ('canon_timepoints', 'canon', 'canon_timepoint', 'PERMANENT'),
  -- Long-term narrative control
  ('narrative_obligations', 'narrative_control', 'obligation', 'PERMANENT'),
  ('obligation_dependencies', 'narrative_control', 'obligation_dependency', 'PERMANENT'),
  ('obligation_evidence', 'narrative_control', 'obligation_evidence', 'PERMANENT'),
  ('foreshadows', 'narrative_control', 'foreshadow', 'PERMANENT'),
  ('foreshadow_windows', 'narrative_control', 'foreshadow_window', 'PERMANENT'),
  ('character_arc_states', 'narrative_control', 'character_arc_state', 'PERMANENT'),
  ('mystery_states', 'narrative_control', 'mystery_state', 'PERMANENT'),
  ('reader_desires', 'narrative_control', 'reader_desire', 'PERMANENT'),
  ('reader_desire_events', 'narrative_control', 'reader_desire_event', 'PERMANENT'),
  ('reader_desire_evidence', 'narrative_control', 'reader_desire_evidence', 'PERMANENT'),
  ('narrative_taste_profile_index', 'quality_evidence', 'narrative_taste_profile', 'POLICY'),
  ('artifact_schemas', 'artifact', 'artifact_schemas_registry', 'PERMANENT'),
  -- Typed dependency / change / retcon
  ('dependency_edges', 'dependency', 'dependency_edge', 'PERMANENT'),
  ('change_requests', 'change', 'change_request', 'PERMANENT'),
  ('impact_sets', 'change', 'impact_set', 'POLICY'),
  ('retcon_plans', 'change', 'retcon_plan', 'PERMANENT'),
  ('stale_queue', 'change', 'stale_queue', 'POLICY'),
  -- Production runtime
  ('jobs', 'runtime', 'job', 'AUDIT'),
  ('job_steps', 'runtime', 'job_step', 'AUDIT'),
  ('job_leases', 'runtime', 'job_lease', 'AUDIT'),
  ('job_checkpoints', 'runtime', 'job_checkpoint', 'AUDIT'),
  ('production_events', 'runtime', 'production_event', 'AUDIT'),
  -- Model & budget
  ('model_runs', 'runtime_policy', 'model_run', 'AUDIT'),
  ('model_responses', 'runtime_policy', 'model_response', 'POLICY'),
  ('model_parse_results', 'runtime_policy', 'model_parse_result', 'POLICY'),
  ('model_usage', 'runtime_policy', 'model_usage', 'AUDIT'),
  ('budget_reservations', 'runtime_policy', 'budget_reservation', 'AUDIT'),
  ('prompt_registry', 'runtime_policy', 'prompt_registry', 'AUDIT'),
  ('quality_trend_projection', 'quality_evidence', 'quality_vector', 'POLICY'),
  ('model_cost_ledger', 'runtime_policy', 'model_cost_ledger', 'AUDIT'),
  -- Search / projection (derived)
  ('chapter_index', 'search_projection', 'chapter_index', 'POLICY'),
  ('entity_index', 'search_projection', 'entity_index', 'POLICY'),
  ('volume_health_projection', 'health_projection', 'volume_health_projection', 'POLICY'),
  ('book_health_projection', 'health_projection', 'book_health_projection', 'POLICY'),
  ('production_timeline_projection', 'production_projection', 'production_timeline_projection', 'POLICY'),
  ('reader_desire_health_projection', 'health_projection', 'reader_desire_health_projection', 'POLICY'),
  ('chapter_production_budgets', 'runtime_policy', 'production_budget', 'AUDIT');

-- Ensure updated long-term tables never silently drop rows against the frozen
-- projection semantics: committed rubric/authority rows are immutable in place.
CREATE TRIGGER IF NOT EXISTS canonical_head_immutable
BEFORE UPDATE OF payload_hash ON canonical_heads
BEGIN
  SELECT RAISE(ABORT, 'canonical head payload_hash is immutable');
END;

-- ---------------------------------------------------------------------------
-- 11. Copy-Project policy registry (single authority for the B01-T08 remap)
-- ---------------------------------------------------------------------------
-- `COPY_REMAP`  = authoritative long-term rows are copied and their project_id
--                 is rewritten to the copy's id.
-- `COPY_CLEAR`  = runtime/lease/telemetry/derived rows are cleared (a copy starts
--                 its own runtime and rebuilds its own projections). This also
--                 avoids a UNIQUE(project_id, idempotency_key) collision and a
--                 dangling FK to a deleted job when the copy later re-runs.
--
-- This replaces the hand-maintained Rust COPY_* constants: adding a long-term
-- table now means one migration edit here (CREATE TABLE + this row), and
-- remap_copy reads the policy from this registry at runtime. The M0A audit
-- tables (commands/commit_receipts/gc_receipts/project_command_receipts/
-- copy_receipts) and project_meta are handled explicitly in remap_copy and are
-- intentionally not listed here.
--
-- The table_name is deliberately NOT FK-constrained to table_family_registry:
-- copy policy is a standalone per-table declaration, and its integrity is
-- guarded by the B01-T08 drift test (every project_id table must have a policy)
-- rather than by a family FK that would block a re-application of the alignment
-- DELETEs.

CREATE TABLE IF NOT EXISTS copy_policy_registry (
  table_name TEXT PRIMARY KEY NOT NULL,
  copy_policy TEXT NOT NULL CHECK (copy_policy IN ('COPY_REMAP', 'COPY_CLEAR'))
);

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy) VALUES
  -- Project & authoritative structural state → remap
  ('artifacts', 'COPY_REMAP'),
  ('canonical_heads', 'COPY_REMAP'),
  ('project_settings', 'COPY_REMAP'),
  ('books', 'COPY_REMAP'),
  ('volumes', 'COPY_REMAP'),
  ('arcs', 'COPY_REMAP'),
  ('chapters', 'COPY_REMAP'),
  ('artifact_schemas', 'COPY_REMAP'),
  -- Planning projections (derived but carry book/volume/arc/chapter identity) → remap
  ('book_plan_index', 'COPY_REMAP'),
  ('volume_plan_index', 'COPY_REMAP'),
  ('arc_plan_index', 'COPY_REMAP'),
  ('chapter_plan_index', 'COPY_REMAP'),
  ('planning_state_ledger_index', 'COPY_REMAP'),
  -- Canon (authoritative) → remap
  ('canon_entities', 'COPY_REMAP'),
  ('canon_aliases', 'COPY_REMAP'),
  ('canon_facts', 'COPY_REMAP'),
  ('canon_relations', 'COPY_REMAP'),
  ('canon_knowledge', 'COPY_REMAP'),
  ('canon_events', 'COPY_REMAP'),
  ('canon_evidence', 'COPY_REMAP'),
  ('canon_timepoints', 'COPY_REMAP'),
  -- Long-term narrative control (authoritative) → remap
  ('narrative_obligations', 'COPY_REMAP'),
  ('obligation_dependencies', 'COPY_REMAP'),
  ('obligation_evidence', 'COPY_REMAP'),
  ('foreshadows', 'COPY_REMAP'),
  ('foreshadow_windows', 'COPY_REMAP'),
  ('character_arc_states', 'COPY_REMAP'),
  ('mystery_states', 'COPY_REMAP'),
  ('reader_desires', 'COPY_REMAP'),
  ('reader_desire_events', 'COPY_REMAP'),
  ('reader_desire_evidence', 'COPY_REMAP'),
  ('narrative_taste_profile_index', 'COPY_REMAP'),
  -- Typed dependency / change / retcon (authoritative) → remap
  ('dependency_edges', 'COPY_REMAP'),
  ('change_requests', 'COPY_REMAP'),
  ('impact_sets', 'COPY_REMAP'),
  ('retcon_plans', 'COPY_REMAP'),
  ('stale_queue', 'COPY_REMAP'),
  -- Model/budget policy (authoritative) → remap
  ('prompt_registry', 'COPY_REMAP'),
  ('chapter_production_budgets', 'COPY_REMAP'),
  -- Runtime / lease / telemetry / derived → clear
  ('project_runtime_state', 'COPY_CLEAR'),
  ('jobs', 'COPY_CLEAR'),
  ('job_steps', 'COPY_CLEAR'),
  ('job_leases', 'COPY_CLEAR'),
  ('job_checkpoints', 'COPY_CLEAR'),
  ('production_events', 'COPY_CLEAR'),
  ('model_runs', 'COPY_CLEAR'),
  ('model_responses', 'COPY_CLEAR'),
  ('model_parse_results', 'COPY_CLEAR'),
  ('model_usage', 'COPY_CLEAR'),
  ('budget_reservations', 'COPY_CLEAR'),
  ('model_cost_ledger', 'COPY_CLEAR'),
  ('chapter_index', 'COPY_CLEAR'),
  ('entity_index', 'COPY_CLEAR'),
  ('volume_health_projection', 'COPY_CLEAR'),
  ('book_health_projection', 'COPY_CLEAR'),
  ('production_timeline_projection', 'COPY_CLEAR'),
  ('reader_desire_health_projection', 'COPY_CLEAR'),
  ('quality_trend_projection', 'COPY_CLEAR');

-- ---------------------------------------------------------------------------
-- 11. Key indexes (V2.1 §21.3 must be established in the first-version migration)
-- ---------------------------------------------------------------------------
-- Every production Query must be served by an index rather than loading a whole
-- table into TS and filtering. The composite indexes include project_id/scope
-- and paging columns used by server-side pagination.

-- artifacts: already indexed in the M0A layer; ensure the project/version paging
-- path is covered.
CREATE INDEX IF NOT EXISTS idx_artifacts_project_paging
  ON artifacts(project_id, artifact_type, artifact_status, version);

-- canonical heads: per scope lookup.
CREATE INDEX IF NOT EXISTS idx_canonical_heads_scope
  ON canonical_heads(project_id, artifact_type, scope_level, scope_key);

-- Canon facts: subjects with time validity, and source-chapter provenance.
CREATE INDEX IF NOT EXISTS idx_canon_facts_subject_time
  ON canon_facts(subject_id, predicate, valid_from_chapter_id, valid_to_chapter_id);
CREATE INDEX IF NOT EXISTS idx_canon_facts_source
  ON canon_facts(source_artifact_id, source_version);

-- Canon relations: both direction lookups.
CREATE INDEX IF NOT EXISTS idx_canon_relations_subject
  ON canon_relations(subject_id, predicate, object_id);
CREATE INDEX IF NOT EXISTS idx_canon_relations_object
  ON canon_relations(object_id, predicate, subject_id);

-- Canon knowledge: knower + epistemic state + fact.
CREATE INDEX IF NOT EXISTS idx_canon_knowledge_knower
  ON canon_knowledge(knower_id, epistemic_state, fact_id);

-- Canon events: by chapter and type.
CREATE INDEX IF NOT EXISTS idx_canon_events_chapter
  ON canon_events(chapter_id, event_type);

-- Dependency edges: both ref directions with type/scope.
CREATE INDEX IF NOT EXISTS idx_dependency_edges_source
  ON dependency_edges(source_ref, dependency_type, scope);
CREATE INDEX IF NOT EXISTS idx_dependency_edges_target
  ON dependency_edges(target_ref, dependency_type, scope);

-- Narrative obligations: aging + resolution scheduling.
CREATE INDEX IF NOT EXISTS idx_obligations_lifecycle
  ON narrative_obligations(lifecycle, scope, must_resolve_before_chapter);

-- Foreshadows: recovery windows.
CREATE INDEX IF NOT EXISTS idx_foreshadows_status_window
  ON foreshadows(status, recover_window_end_chapter);

-- Reader desires: aging + payoff windows.
CREATE INDEX IF NOT EXISTS idx_reader_desires_status
  ON reader_desires(status, desire_type, expected_payoff_chapter);
CREATE INDEX IF NOT EXISTS idx_reader_desire_events_desire
  ON reader_desire_events(desire_id, chapter_id);

-- Chapters: volume + chapter_no for the frozen chapter tree.
CREATE INDEX IF NOT EXISTS idx_chapters_volume_no
  ON chapters(volume_id, chapter_no);

-- Model runs: step-level status and job paging.
CREATE INDEX IF NOT EXISTS idx_model_runs_step_status
  ON model_runs(step_id, status);
CREATE INDEX IF NOT EXISTS idx_model_runs_job_paging
  ON model_runs(job_id, created_at);

-- Production events: timeline ordering.
CREATE INDEX IF NOT EXISTS idx_production_events_created
  ON production_events(project_id, created_at);

-- Project/scope/paging composite indexes for the long lists.
CREATE INDEX IF NOT EXISTS idx_jobs_project_paging
  ON jobs(project_id, status, created_at);
CREATE INDEX IF NOT EXISTS idx_artifact_schemas_project
  ON artifact_schemas(project_id, artifact_type);
CREATE INDEX IF NOT EXISTS idx_stale_queue_project
  ON stale_queue(project_id, processed, created_at);
CREATE INDEX IF NOT EXISTS idx_change_requests_project
  ON change_requests(project_id, status, created_at);
CREATE INDEX IF NOT EXISTS idx_model_cost_ledger_project
  ON model_cost_ledger(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_production_events_job
  ON production_events(job_id, created_at);
CREATE INDEX IF NOT EXISTS idx_budget_reservations_project
  ON budget_reservations(project_id, status);
CREATE INDEX IF NOT EXISTS idx_arcs_volume
  ON arcs(volume_id, arc_no);

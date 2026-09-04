-- B13 Long-form Health: additive query indexes only.
-- Migrations 0001-0011 are frozen and must remain byte-identical.

CREATE INDEX IF NOT EXISTS idx_b13_obligations_project_lifecycle_deadline
  ON narrative_obligations(project_id, lifecycle, must_resolve_before_chapter, obligation_id);

CREATE INDEX IF NOT EXISTS idx_b13_foreshadows_project_status_window
  ON foreshadows(project_id, status, recover_window_end_chapter, foreshadow_id);

CREATE INDEX IF NOT EXISTS idx_b13_reader_desires_project_status_payoff
  ON reader_desires(project_id, status, expected_payoff_chapter, desire_id);

CREATE INDEX IF NOT EXISTS idx_b13_chapters_project_status_number
  ON chapters(project_id, status, chapter_no, chapter_id);

CREATE INDEX IF NOT EXISTS idx_b13_model_runs_project_group_status_time
  ON model_runs(project_id, capability, provider, model, status, created_at, model_run_id);

CREATE INDEX IF NOT EXISTS idx_b13_model_usage_project_recorded_run
  ON model_usage(project_id, recorded_at, model_run_id);

CREATE INDEX IF NOT EXISTS idx_b13_quality_project_chapter_sample
  ON quality_trend_projection(project_id, sample_chapter_no, sampled_at, sample_chapter_id);

CREATE INDEX IF NOT EXISTS idx_b13_ledger_project_revision_volume
  ON planning_state_ledger_index(project_id, generated_revision, volume_id);

CREATE INDEX IF NOT EXISTS idx_b13_task_reality_project_type_time
  ON state_deltas(project_id, state_type, updated_at, state_id);

CREATE INDEX IF NOT EXISTS idx_b13_reservations_project_status_time
  ON budget_reservations(project_id, status, created_at, reservation_id);

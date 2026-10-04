-- B-R2-03 R4: precomputed, rebuildable health route/task projections.
-- Migrations 0001-0012 remain byte-frozen. Queries page these relational rows;
-- the formal writer refreshes only scopes marked dirty in its transaction.

CREATE TABLE IF NOT EXISTS health_projection_state (
  project_id TEXT NOT NULL PRIMARY KEY,
  status TEXT NOT NULL CHECK (status IN ('READY','DIRTY','REBUILD_REQUIRED')),
  source_revision INTEGER NOT NULL CHECK (source_revision >= -1),
  updated_at TEXT NOT NULL,
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS health_projection_dirty (
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  marked_at TEXT NOT NULL,
  PRIMARY KEY (project_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS health_route_scope_projection (
  project_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  volume_no INTEGER NOT NULL,
  volume_title TEXT NOT NULL,
  plan_state TEXT NOT NULL,
  is_stale INTEGER NOT NULL CHECK (is_stale IN (0,1)),
  is_unavailable INTEGER NOT NULL CHECK (is_unavailable IN (0,1)),
  capacity INTEGER,
  required_tasks_remaining INTEGER,
  blocked_required_tasks INTEGER,
  blocked_optional_tasks INTEGER,
  task_summary_available INTEGER NOT NULL CHECK (task_summary_available IN (0,1)),
  source_revision INTEGER NOT NULL CHECK (source_revision >= 0),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (project_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS health_task_scope_index (
  project_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  volume_id TEXT NOT NULL,
  task_class TEXT NOT NULL,
  PRIMARY KEY (project_id, task_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id),
  FOREIGN KEY (volume_id) REFERENCES volumes(volume_id)
);

CREATE TABLE IF NOT EXISTS health_issue_projection (
  project_id TEXT NOT NULL,
  issue_id TEXT NOT NULL,
  factor_code TEXT NOT NULL,
  severity TEXT NOT NULL CHECK (severity IN ('risk','attention')),
  title TEXT NOT NULL,
  detail TEXT NOT NULL,
  location TEXT NOT NULL,
  due_chapter INTEGER,
  recommended_destination TEXT NOT NULL,
  source_kind TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  severity_rank INTEGER NOT NULL CHECK (severity_rank IN (0,1)),
  factor_rank INTEGER NOT NULL,
  due_sort INTEGER NOT NULL,
  volume_id TEXT NOT NULL,
  source_revision INTEGER NOT NULL CHECK (source_revision >= 0),
  PRIMARY KEY (project_id, issue_id, volume_id),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE INDEX IF NOT EXISTS idx_health_issue_page
  ON health_issue_projection(project_id, severity_rank, factor_rank, due_sort, issue_id, volume_id);
CREATE INDEX IF NOT EXISTS idx_health_issue_factor_page
  ON health_issue_projection(project_id, factor_code, severity_rank, due_sort, issue_id, volume_id);
CREATE INDEX IF NOT EXISTS idx_health_task_scope_task
  ON health_task_scope_index(project_id, task_id, volume_id);
CREATE INDEX IF NOT EXISTS idx_health_projection_dirty_project
  ON health_projection_dirty(project_id, volume_id);
CREATE INDEX IF NOT EXISTS idx_health_route_summary_project
  ON health_route_scope_projection(project_id, is_stale, capacity, required_tasks_remaining);

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only)
VALUES ('health_issue_projection', 'health_issue_projection', 'B13::HealthProjectionMaintenance', 1);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class)
VALUES
  ('health_projection_state', 'health_projection_state', 'health_issue_projection', 'POLICY'),
  ('health_projection_dirty', 'health_projection_dirty', 'health_issue_projection', 'POLICY'),
  ('health_route_scope_projection', 'health_projection', 'health_issue_projection', 'POLICY'),
  ('health_task_scope_index', 'health_projection', 'health_issue_projection', 'POLICY'),
  ('health_issue_projection', 'health_projection', 'health_issue_projection', 'POLICY');

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy)
VALUES
  ('health_projection_state', 'COPY_REMAP'),
  ('health_projection_dirty', 'COPY_REMAP'),
  ('health_route_scope_projection', 'COPY_REMAP'),
  ('health_task_scope_index', 'COPY_REMAP'),
  ('health_issue_projection', 'COPY_REMAP');

CREATE TRIGGER IF NOT EXISTS health_projection_state_after_project_insert
AFTER INSERT ON project_meta
BEGIN
  INSERT OR IGNORE INTO health_projection_state(project_id,status,source_revision,updated_at)
  VALUES (NEW.project_id,'READY',NEW.revision,NEW.updated_at);
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_volume_plan_insert
AFTER INSERT ON volume_plan_index
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (NEW.project_id,NEW.volume_id,'VOLUME_PLAN_INDEX',NEW.updated_at)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.updated_at WHERE project_id=NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_volume_plan_update
AFTER UPDATE ON volume_plan_index
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (NEW.project_id,NEW.volume_id,'VOLUME_PLAN_INDEX',NEW.updated_at)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.updated_at WHERE project_id=NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_volume_plan_delete
AFTER DELETE ON volume_plan_index
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (OLD.project_id,OLD.volume_id,'VOLUME_PLAN_INDEX',CURRENT_TIMESTAMP)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP WHERE project_id=OLD.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_planning_projection_insert
AFTER INSERT ON volume_planning_state_projection
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (NEW.project_id,NEW.volume_id,'PLANNING_PROJECTION',NEW.updated_at)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.updated_at WHERE project_id=NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_planning_projection_update
AFTER UPDATE ON volume_planning_state_projection
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (NEW.project_id,NEW.volume_id,'PLANNING_PROJECTION',NEW.updated_at)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.updated_at WHERE project_id=NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_planning_projection_delete
AFTER DELETE ON volume_planning_state_projection
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (OLD.project_id,OLD.volume_id,'PLANNING_PROJECTION',CURRENT_TIMESTAMP)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP WHERE project_id=OLD.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_canonical_volume_plan_insert
AFTER INSERT ON canonical_heads WHEN NEW.artifact_type='VolumePlan'
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT NEW.project_id,CASE WHEN json_valid(NEW.scope_key)=1 THEN COALESCE(json_extract(NEW.scope_key,'$.volumeId'),NEW.scope_key) ELSE NEW.scope_key END,'CANONICAL_VOLUME_PLAN',CURRENT_TIMESTAMP
  WHERE EXISTS (SELECT 1 FROM volumes v WHERE v.project_id=NEW.project_id AND v.volume_id=CASE WHEN json_valid(NEW.scope_key)=1 THEN COALESCE(json_extract(NEW.scope_key,'$.volumeId'),NEW.scope_key) ELSE NEW.scope_key END)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP WHERE project_id=NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_canonical_volume_plan_delete
AFTER DELETE ON canonical_heads WHEN OLD.artifact_type='VolumePlan'
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT OLD.project_id,CASE WHEN json_valid(OLD.scope_key)=1 THEN COALESCE(json_extract(OLD.scope_key,'$.volumeId'),OLD.scope_key) ELSE OLD.scope_key END,'CANONICAL_VOLUME_PLAN',CURRENT_TIMESTAMP
  WHERE EXISTS (SELECT 1 FROM volumes v WHERE v.project_id=OLD.project_id AND v.volume_id=CASE WHEN json_valid(OLD.scope_key)=1 THEN COALESCE(json_extract(OLD.scope_key,'$.volumeId'),OLD.scope_key) ELSE OLD.scope_key END)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP WHERE project_id=OLD.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_canonical_volume_plan_update
AFTER UPDATE ON canonical_heads WHEN NEW.artifact_type='VolumePlan' OR OLD.artifact_type='VolumePlan'
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT NEW.project_id,CASE WHEN json_valid(NEW.scope_key)=1 THEN COALESCE(json_extract(NEW.scope_key,'$.volumeId'),NEW.scope_key) ELSE NEW.scope_key END,'CANONICAL_VOLUME_PLAN',CURRENT_TIMESTAMP
  WHERE NEW.artifact_type='VolumePlan' AND EXISTS (SELECT 1 FROM volumes v WHERE v.project_id=NEW.project_id AND v.volume_id=CASE WHEN json_valid(NEW.scope_key)=1 THEN COALESCE(json_extract(NEW.scope_key,'$.volumeId'),NEW.scope_key) ELSE NEW.scope_key END)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT OLD.project_id,CASE WHEN json_valid(OLD.scope_key)=1 THEN COALESCE(json_extract(OLD.scope_key,'$.volumeId'),OLD.scope_key) ELSE OLD.scope_key END,'CANONICAL_VOLUME_PLAN',CURRENT_TIMESTAMP
  WHERE OLD.artifact_type='VolumePlan' AND EXISTS (SELECT 1 FROM volumes v WHERE v.project_id=OLD.project_id AND v.volume_id=CASE WHEN json_valid(OLD.scope_key)=1 THEN COALESCE(json_extract(OLD.scope_key,'$.volumeId'),OLD.scope_key) ELSE OLD.scope_key END)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP
  WHERE project_id IN (NEW.project_id,OLD.project_id);
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_task_state_insert
AFTER INSERT ON state_deltas WHEN NEW.state_type='volume.task.progress'
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT project_id,volume_id,'TASK_PROGRESS',NEW.updated_at FROM health_task_scope_index
  WHERE project_id=NEW.project_id AND task_id=NEW.state_id
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.updated_at
  WHERE project_id=NEW.project_id AND EXISTS (SELECT 1 FROM health_task_scope_index WHERE project_id=NEW.project_id AND task_id=NEW.state_id);
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_task_state_update
AFTER UPDATE ON state_deltas WHEN NEW.state_type='volume.task.progress' OR OLD.state_type='volume.task.progress'
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT project_id,volume_id,'TASK_PROGRESS',NEW.updated_at FROM health_task_scope_index
  WHERE project_id=NEW.project_id AND task_id=NEW.state_id
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.updated_at
  WHERE project_id=NEW.project_id AND EXISTS (SELECT 1 FROM health_task_scope_index WHERE project_id=NEW.project_id AND task_id=NEW.state_id);
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_task_state_delete
AFTER DELETE ON state_deltas WHEN OLD.state_type='volume.task.progress'
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT project_id,volume_id,'TASK_PROGRESS',CURRENT_TIMESTAMP FROM health_task_scope_index
  WHERE project_id=OLD.project_id AND task_id=OLD.state_id
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP
  WHERE project_id=OLD.project_id AND EXISTS (SELECT 1 FROM health_task_scope_index WHERE project_id=OLD.project_id AND task_id=OLD.state_id);
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_stale_queue_insert
AFTER INSERT ON stale_queue
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT project_id,volume_id,'STALE_QUEUE',CURRENT_TIMESTAMP FROM volume_plan_index
  WHERE project_id=NEW.project_id AND (volume_id=NEW.target_ref OR plan_artifact_id=NEW.target_ref)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT h.project_id,CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END,'STALE_QUEUE',CURRENT_TIMESTAMP
  FROM canonical_heads h JOIN volumes v ON v.project_id=h.project_id
    AND v.volume_id=CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END
  WHERE h.project_id=NEW.project_id AND h.artifact_type='VolumePlan'
    AND (h.artifact_id=NEW.target_ref OR h.scope_key=NEW.target_ref
         OR CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END=NEW.target_ref)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP
  WHERE project_id=NEW.project_id AND (
    EXISTS (SELECT 1 FROM volume_plan_index WHERE project_id=NEW.project_id AND (volume_id=NEW.target_ref OR plan_artifact_id=NEW.target_ref))
    OR EXISTS (SELECT 1 FROM canonical_heads h JOIN volumes v ON v.project_id=h.project_id AND v.volume_id=CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END WHERE h.project_id=NEW.project_id AND h.artifact_type='VolumePlan' AND (h.artifact_id=NEW.target_ref OR h.scope_key=NEW.target_ref OR CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END=NEW.target_ref))
  );
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_stale_queue_update
AFTER UPDATE ON stale_queue
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT project_id,volume_id,'STALE_QUEUE',CURRENT_TIMESTAMP FROM volume_plan_index
  WHERE project_id=NEW.project_id AND (volume_id=NEW.target_ref OR plan_artifact_id=NEW.target_ref OR volume_id=OLD.target_ref OR plan_artifact_id=OLD.target_ref)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT h.project_id,CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END,'STALE_QUEUE',CURRENT_TIMESTAMP
  FROM canonical_heads h JOIN volumes v ON v.project_id=h.project_id
    AND v.volume_id=CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END
  WHERE h.project_id=NEW.project_id AND h.artifact_type='VolumePlan'
    AND (h.artifact_id IN (NEW.target_ref,OLD.target_ref) OR h.scope_key IN (NEW.target_ref,OLD.target_ref)
         OR CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END IN (NEW.target_ref,OLD.target_ref))
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP
  WHERE project_id=NEW.project_id AND (
    EXISTS (SELECT 1 FROM volume_plan_index WHERE project_id=NEW.project_id AND (volume_id=NEW.target_ref OR plan_artifact_id=NEW.target_ref OR volume_id=OLD.target_ref OR plan_artifact_id=OLD.target_ref))
    OR EXISTS (SELECT 1 FROM canonical_heads h JOIN volumes v ON v.project_id=h.project_id AND v.volume_id=CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END WHERE h.project_id=NEW.project_id AND h.artifact_type='VolumePlan' AND (h.artifact_id IN (NEW.target_ref,OLD.target_ref) OR h.scope_key IN (NEW.target_ref,OLD.target_ref) OR CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END IN (NEW.target_ref,OLD.target_ref)))
  );
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_stale_queue_delete
AFTER DELETE ON stale_queue
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT project_id,volume_id,'STALE_QUEUE',CURRENT_TIMESTAMP FROM volume_plan_index
  WHERE project_id=OLD.project_id AND (volume_id=OLD.target_ref OR plan_artifact_id=OLD.target_ref)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP WHERE project_id=OLD.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_route_replan_insert
AFTER INSERT ON volume_route_replans
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (NEW.project_id,NEW.volume_id,'ROUTE_REPLAN',NEW.created_at)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.created_at WHERE project_id=NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_route_replan_update
AFTER UPDATE ON volume_route_replans
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (NEW.project_id,NEW.volume_id,'ROUTE_REPLAN',NEW.created_at)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.created_at WHERE project_id=NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_route_replan_delete
AFTER DELETE ON volume_route_replans
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  VALUES (OLD.project_id,OLD.volume_id,'ROUTE_REPLAN',CURRENT_TIMESTAMP)
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP WHERE project_id=OLD.project_id;
END;

CREATE TRIGGER IF NOT EXISTS health_projection_dirty_after_volume_update
AFTER UPDATE OF title,volume_no,status ON volumes
BEGIN
  INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
  SELECT NEW.project_id,NEW.volume_id,'VOLUME_METADATA',NEW.updated_at
  WHERE EXISTS (SELECT 1 FROM volume_plan_index i WHERE i.project_id=NEW.project_id AND i.volume_id=NEW.volume_id)
     OR EXISTS (SELECT 1 FROM canonical_heads h WHERE h.project_id=NEW.project_id AND h.artifact_type='VolumePlan' AND h.scope_key IN (NEW.volume_id,json_object('level','VOLUME','volumeId',NEW.volume_id)))
  ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at;
  UPDATE health_projection_state SET status='DIRTY',updated_at=NEW.updated_at
  WHERE project_id=NEW.project_id AND EXISTS (SELECT 1 FROM health_projection_dirty WHERE project_id=NEW.project_id AND volume_id=NEW.volume_id);
END;

CREATE TRIGGER IF NOT EXISTS health_projection_cleanup_after_volume_delete
AFTER DELETE ON volumes
BEGIN
  DELETE FROM health_issue_projection WHERE project_id=OLD.project_id AND volume_id=OLD.volume_id;
  DELETE FROM health_task_scope_index WHERE project_id=OLD.project_id AND volume_id=OLD.volume_id;
  DELETE FROM health_route_scope_projection WHERE project_id=OLD.project_id AND volume_id=OLD.volume_id;
  DELETE FROM health_projection_dirty WHERE project_id=OLD.project_id AND volume_id=OLD.volume_id;
  UPDATE health_projection_state SET status='DIRTY',updated_at=CURRENT_TIMESTAMP WHERE project_id=OLD.project_id;
END;

-- Existing projects are explicitly rebuilt by ProjectManager after migration;
-- they are never silently served from an absent or stale projection.
INSERT OR IGNORE INTO health_projection_state(project_id,status,source_revision,updated_at)
SELECT project_id,'REBUILD_REQUIRED',-1,updated_at FROM project_meta;

INSERT OR IGNORE INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
SELECT project_id,volume_id,'MIGRATION_REBUILD',CURRENT_TIMESTAMP FROM volume_plan_index;

INSERT OR IGNORE INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
SELECT h.project_id,CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END,'MIGRATION_REBUILD',CURRENT_TIMESTAMP
FROM canonical_heads h JOIN volumes v ON v.project_id=h.project_id
  AND v.volume_id=CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END
WHERE h.artifact_type='VolumePlan';

UPDATE health_projection_state SET status='REBUILD_REQUIRED',source_revision=-1
WHERE EXISTS (SELECT 1 FROM health_projection_dirty d WHERE d.project_id=health_projection_state.project_id);

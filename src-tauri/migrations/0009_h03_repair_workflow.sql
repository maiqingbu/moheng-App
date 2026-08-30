-- H03 support workflow only.  These tables hold a durable proposal,
-- authorization and execution receipt; they are not Narrative authority and
-- must never be used as a shadow Canon/Plan/Task store.
CREATE TABLE IF NOT EXISTS repair_plans (
  repair_plan_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  based_on_project_revision INTEGER NOT NULL CHECK (based_on_project_revision >= 0),
  diagnosis_json TEXT NOT NULL,
  finding_refs_json TEXT NOT NULL,
  target_authority_owner TEXT NOT NULL,
  proposed_operations_json TEXT NOT NULL,
  affected_artifacts_json TEXT NOT NULL,
  affected_scopes_json TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  requires_backup INTEGER NOT NULL CHECK (requires_backup IN (0,1)),
  expected_postconditions_json TEXT NOT NULL,
  operation_digest TEXT NOT NULL CHECK (length(operation_digest) = 64),
  status TEXT NOT NULL DEFAULT 'PROPOSED',
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repair_plans_project_revision
  ON repair_plans(project_id, based_on_project_revision, created_at DESC);

CREATE TABLE IF NOT EXISTS repair_authorizations (
  authorization_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  repair_plan_id TEXT NOT NULL,
  based_on_project_revision INTEGER NOT NULL CHECK (based_on_project_revision >= 0),
  operation_digest TEXT NOT NULL CHECK (length(operation_digest) = 64),
  approved_by_user INTEGER NOT NULL CHECK (approved_by_user IN (0,1)),
  issued_at TEXT NOT NULL,
  consumed_at TEXT,
  FOREIGN KEY (repair_plan_id) REFERENCES repair_plans(repair_plan_id)
);

CREATE INDEX IF NOT EXISTS idx_repair_authorizations_plan
  ON repair_authorizations(project_id, repair_plan_id, issued_at DESC);

CREATE TABLE IF NOT EXISTS repair_execution_receipts (
  receipt_ref TEXT PRIMARY KEY NOT NULL,
  repair_plan_id TEXT NOT NULL,
  authorization_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  before_revision INTEGER NOT NULL CHECK (before_revision >= 0),
  after_revision INTEGER NOT NULL CHECK (after_revision >= 0),
  operation_digest TEXT NOT NULL CHECK (length(operation_digest) = 64),
  owner TEXT NOT NULL,
  owner_receipt_ref TEXT,
  executed_at TEXT NOT NULL,
  status TEXT NOT NULL,
  postconditions_json TEXT NOT NULL,
  FOREIGN KEY (repair_plan_id) REFERENCES repair_plans(repair_plan_id),
  FOREIGN KEY (authorization_id) REFERENCES repair_authorizations(authorization_id)
);

INSERT OR IGNORE INTO copy_policy_registry(table_name, copy_policy)
  VALUES ('repair_plans', 'COPY_CLEAR'), ('repair_authorizations', 'COPY_CLEAR'),
         ('repair_execution_receipts', 'COPY_CLEAR');

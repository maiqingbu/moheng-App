-- M0A ProjectCommand + CopyProject 权威持久化表。
--
-- project_command_receipts: 为每个通过权威写协议执行的 Project 生命周期命令
-- 持久化一份可验证 Receipt（request digest + before/after revision + result
-- projectId + receipt digest），允许 IPC 超时后安全重试（同 key + 同 digest 返回
-- 原 Receipt；同 key + 不同 digest 拒绝）。
--
-- copy_receipts: 记录一次 Copy Project（duplicate）动作。副本不复制源项目的
-- runtime/audit Receipt（它们不可变、且属于源身份），改为写入一条副本自身的
-- CopyReceipt 作为复制审计与 provenance。

CREATE TABLE IF NOT EXISTS project_command_receipts (
  receipt_id TEXT PRIMARY KEY NOT NULL,
  project_id TEXT NOT NULL,
  result_project_id TEXT NOT NULL,
  command_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  command_type TEXT NOT NULL,
  before_revision INTEGER NOT NULL CHECK (before_revision >= 0),
  after_revision INTEGER NOT NULL CHECK (after_revision >= 0),
  request_digest TEXT NOT NULL CHECK (length(request_digest) = 64),
  receipt_json TEXT NOT NULL,
  receipt_digest TEXT NOT NULL CHECK (length(receipt_digest) = 64),
  committed_at TEXT NOT NULL,
  UNIQUE (project_id, idempotency_key),
  FOREIGN KEY (project_id) REFERENCES project_meta(project_id)
);

CREATE TABLE IF NOT EXISTS copy_receipts (
  receipt_id TEXT PRIMARY KEY NOT NULL,
  source_project_id TEXT NOT NULL,
  target_project_id TEXT NOT NULL,
  copied_at TEXT NOT NULL,
  receipt_json TEXT NOT NULL,
  receipt_digest TEXT NOT NULL CHECK (length(receipt_digest) = 64)
);

INSERT OR IGNORE INTO authority_registry(authority_key, authoritative_table, write_owner, projection_only) VALUES
  ('project_command_receipt', 'project_command_receipts', 'CommitService', 0),
  ('copy_receipt', 'copy_receipts', 'ProjectManager', 0);

INSERT OR IGNORE INTO table_family_registry(table_name, family, authority_key, retention_class) VALUES
  ('project_command_receipts', 'receipt', 'project_command_receipt', 'PERMANENT'),
  ('copy_receipts', 'receipt', 'copy_receipt', 'PERMANENT');

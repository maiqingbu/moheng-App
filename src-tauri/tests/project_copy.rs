//! P0-02: a real, data-bearing project duplicate must remap every authoritative
//! `project_id` table, drop the source's runtime/audit artefacts, write a
//! CopyReceipt, and produce a copy that is FK-clean and isolated.
//!
//! The source is seeded directly via SQL so we can precisely construct the
//! "committed artifact + dependency + canonical head + command + CommitReceipt +
//! GCReceipt" shape a long-running project would own, matching the复现 report's
//! SQL-level reproduction rather than depending on candidate-version plumbing.

use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use serde_json::json;
use tempfile::tempdir;

const NOW: &str = "2026-08-25T00:00:00Z";

/// Seeds the source project with fully-formed authoritative rows: two artifact
/// versions, a dependency edge, a canonical head, a command, a CommitReceipt and
/// a GCReceipt. Everything is written with the *source* project id so the remap
/// contract has real cross-table data to rewrite.
fn seed_source(connection: &libsql_rusqlite::Connection, source_id: &str, now: &str) {
    let payload = json!({"text": "a\r\nb"});
    let payload_hash = sha256_canonical(&payload).unwrap();
    let payload_json = serde_json::to_string(&payload).unwrap();
    let provenance = serde_json::to_string(&json!({
        "createdBy": "test",
        "commandId": "command-1",
        "projectId": source_id
    }))
    .unwrap();

    // Two committed artifact versions under the source id.
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, semantic_lifecycle, authority_level, payload_hash, provenance_json, created_at, committed_at) VALUES ('a1', ?1, 'ChapterCandidate', 'CHAPTER', 'chapter-1', 1, 'm0a.artifact.v1', 'COMMITTED', 'CANON', 'A', ?2, ?3, ?4, ?4)",
            libsql_rusqlite::params![source_id, payload_hash, provenance, now],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, semantic_lifecycle, authority_level, payload_hash, provenance_json, created_at, committed_at) VALUES ('a1', ?1, 'ChapterCandidate', 'CHAPTER', 'chapter-1', 2, 'm0a.artifact.v1', 'COMMITTED', 'CANON', 'A', ?2, ?3, ?4, ?4)",
            libsql_rusqlite::params![source_id, payload_hash, provenance, now],
        )
        .unwrap();
    for version in [1, 2] {
        connection
            .execute(
                "INSERT INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES ('a1', ?1, ?2, ?3)",
                libsql_rusqlite::params![version, payload_json, payload_hash],
            )
            .unwrap();
    }
    // A dependency edge from a1v2 -> a1v1.
    connection
        .execute(
            "INSERT INTO artifact_dependencies(source_artifact_id, source_version, target_artifact_id, target_version, target_payload_hash) VALUES ('a1', 2, 'a1', 1, ?1)",
            [&payload_hash],
        )
        .unwrap();
    // Canonical head for the project.
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id, artifact_type, scope_level, scope_key, artifact_id, version, payload_hash) VALUES (?1, 'ChapterCandidate', 'CHAPTER', 'chapter-1', 'a1', 2, ?2)",
            libsql_rusqlite::params![source_id, payload_hash],
        )
        .unwrap();
    // Command row.
    connection
        .execute(
            "INSERT INTO commands(project_id, command_id, idempotency_key, request_digest, status, created_at) VALUES (?1, 'command-1', 'key-1', ?2, 'COMMITTED', ?3)",
            libsql_rusqlite::params![source_id, payload_hash, now],
        )
        .unwrap();
    // CommitReceipt (json + digest over a stable unsigned form).
    let mut receipt_body = json!({
        "receiptId": format!("receipt:{source_id}"),
        "commitId": "commit-1",
        "projectId": source_id,
        "commandId": "command-1",
        "idempotencyKey": "key-1",
        "beforeRevision": 0,
        "afterRevision": 1,
        "bundleDigest": payload_hash,
        "committedArtifactRefs": [],
        "headChanges": [],
        "appliedDeltaCounts": {"artifacts": 2},
        "gateCertificateRefs": [],
        "committedAt": now
    });
    receipt_body["receiptDigest"] = json!(sha256_canonical(&receipt_body).unwrap());
    let receipt_json = serde_json::to_string(&receipt_body).unwrap();
    let receipt_digest = receipt_body["receiptDigest"].as_str().unwrap().to_string();
    connection
        .execute(
            "INSERT INTO commit_receipts(receipt_id, project_id, commit_id, command_id, idempotency_key, before_revision, after_revision, bundle_digest, receipt_json, receipt_digest, committed_at) VALUES (?1, ?2, 'commit-1', 'command-1', 'key-1', 0, 1, ?3, ?4, ?5, ?6)",
            libsql_rusqlite::params![
                format!("receipt:{source_id}"),
                source_id,
                payload_hash,
                receipt_json,
                receipt_digest,
                now
            ],
        )
        .unwrap();
    // GCReceipt.
    let mut gc_body = json!({
        "receiptId": format!("gc:{source_id}:{now}"),
        "projectId": source_id,
        "policyDigest": "policy",
        "deletedCandidateCount": 1,
        "createdAt": now
    });
    gc_body["receiptDigest"] = json!(sha256_canonical(&gc_body).unwrap());
    let gc_digest = gc_body["receiptDigest"].as_str().unwrap().to_string();
    connection
        .execute(
            "INSERT INTO gc_receipts(receipt_id, project_id, policy_digest, deleted_candidate_count, created_at, receipt_digest) VALUES (?1, ?2, 'policy', 1, ?3, ?4)",
            libsql_rusqlite::params![
                format!("gc:{source_id}:{now}"),
                source_id,
                now,
                gc_digest
            ],
        )
        .unwrap();
}

/// P0-02: duplicate of a project that owns committed artifacts, a dependency, a
/// canonical head, a command, a CommitReceipt and a GCReceipt must remap those
/// project_id rows to the copy's id, drop the source's runtime/audit artefacts,
/// write a CopyReceipt, and leave a FK-clean copy isolated from the source.
#[test]
fn duplicate_remaps_real_authority_and_stays_isolated() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());

    let connection = manager.create("src", "源项目", NOW).unwrap();
    seed_source(&connection, "src", NOW);
    drop(connection);

    manager
        .duplicate("src", "copy", "2026-08-25T00:00:01Z")
        .unwrap();

    // The copy owns authoritative rows under its own id.
    let copy_conn = manager.read_connection("copy", NOW).unwrap();
    let copy_meta: (String, i64) = copy_conn
        .query_row(
            "SELECT project_id, revision FROM project_meta WHERE project_id = 'copy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(copy_meta.0, "copy");
    assert_eq!(copy_meta.1, 0, "copy starts at revision 0");
    let copy_artifacts: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id = 'copy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(copy_artifacts, 2, "both artifact versions are copied");
    let copy_payloads: i64 = copy_conn
        .query_row("SELECT COUNT(*) FROM artifact_payloads", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        copy_payloads, 2,
        "payload rows are carried with the artifacts"
    );
    let copy_deps: i64 = copy_conn
        .query_row("SELECT COUNT(*) FROM artifact_dependencies", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(copy_deps, 1, "dependency edge is copied");
    let copy_heads: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM canonical_heads WHERE project_id = 'copy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(copy_heads, 1, "canonical head is copied and remapped");
    // Runtime/audit artefacts are NOT copied.
    let copy_commands: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM commands WHERE project_id = 'copy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        copy_commands, 0,
        "commands are runtime artefacts, not copied"
    );
    let copy_receipts: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM commit_receipts WHERE project_id = 'copy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        copy_receipts, 0,
        "commit receipts are source-bound, not copied"
    );
    let copy_gc: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM gc_receipts WHERE project_id = 'copy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(copy_gc, 0, "gc receipts are source-bound, not copied");
    // A CopyReceipt records the duplication.
    let copy_receipt_count: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM copy_receipts WHERE target_project_id = 'copy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        copy_receipt_count, 1,
        "a CopyReceipt records the duplication"
    );
    // FK-clean.
    let fk_present = {
        let mut fk_stmt = copy_conn.prepare("PRAGMA foreign_key_check").unwrap();
        let mut fk_rows = fk_stmt.query([]).unwrap();
        let mut n = 0;
        while fk_rows.next().unwrap().is_some() {
            n += 1;
        }
        n
    };
    drop(copy_conn);
    assert_eq!(fk_present, 0, "copy must be foreign-key clean");

    // Isolation: the source keeps its own identity and receipts.
    let src_conn = manager.read_connection("src", NOW).unwrap();
    let src_meta: String = src_conn
        .query_row(
            "SELECT project_id FROM project_meta WHERE project_id = 'src'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let src_receipts: i64 = src_conn
        .query_row(
            "SELECT COUNT(*) FROM commit_receipts WHERE project_id = 'src'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(src_conn);
    assert_eq!(src_meta, "src");
    assert_eq!(src_receipts, 1, "source keeps its own receipt");

    // Catalog points at the new project dir with a self-contained DB.
    let catalog = CatalogRepository::open(&paths, "2026-08-25T00:00:02Z").unwrap();
    let (copy_path, copy_lifecycle): (String, String) = catalog
        .query_row(
            "SELECT project_path, lifecycle FROM projects WHERE project_id = 'copy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(copy_lifecycle, "ACTIVE");
    assert!(std::path::Path::new(&copy_path)
        .join("project.db")
        .is_file());
}

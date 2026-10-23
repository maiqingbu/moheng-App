//! B01-T08: migration & copy-compatibility for the M0B long-term schema.
//!
//! Verifies the acceptance criteria for the M0B migration:
//!   1. a freshly created project.db owns every §21.2 core table (55 new + the
//!      6 existing M0A tables, plus canon_timepoints required by the Canon model);
//!   2. an old-M0A database migrates to v4 in place, keeping its existing
//!      Receipt/Artifact hashes and passing quick_check/foreign_key_check;
//!   3. repeated open/migration is idempotent (no second application, no backup
//!      storm for an already-up-to-date DB);
//!   4. Copy-Project remaps every long-term table's project_id to the copy id
//!      and resets runtime/lease/audit rows instead of copying them;
//!   5. the authority/table-family registries resolve to real tables (no entry
//!      points at a missing table).

use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::stable_id;
use std::collections::HashSet;
use tempfile::tempdir;

const NOW: &str = "2026-08-26T00:00:00Z";

/// The full set of project.db tables the V2.1 architecture §21.2 mandates.
/// This is the "61 core tables" the acceptance gate counts.
const EXPECTED_CORE_TABLES: &[&str] = &[
    // project & state
    "project_meta",
    "project_runtime_state",
    "project_settings",
    "books",
    "volumes",
    "arcs",
    "chapters",
    // artifact
    "artifacts",
    "canonical_heads",
    "artifact_dependencies",
    "artifact_schemas",
    "artifact_payloads",
    "commit_receipts",
    // long-form planning
    "book_plan_index",
    "volume_plan_index",
    "arc_plan_index",
    "chapter_plan_index",
    "planning_state_ledger_index",
    // canon
    "canon_entities",
    "canon_aliases",
    "canon_facts",
    "canon_relations",
    "canon_knowledge",
    "canon_events",
    "canon_evidence",
    "canon_timepoints",
    // long-term obligations
    "narrative_obligations",
    "obligation_dependencies",
    "obligation_evidence",
    "foreshadows",
    "foreshadow_windows",
    "character_arc_states",
    "mystery_states",
    "reader_desires",
    "reader_desire_events",
    "reader_desire_evidence",
    "narrative_taste_profile_index",
    // dependency & change
    "dependency_edges",
    "change_requests",
    "impact_sets",
    "retcon_plans",
    "stale_queue",
    // production runtime
    "jobs",
    "job_steps",
    "job_leases",
    "job_checkpoints",
    "production_events",
    // model & budget
    "model_runs",
    "model_responses",
    "model_parse_results",
    "model_usage",
    "budget_reservations",
    "prompt_registry",
    "chapter_production_budgets",
    "model_cost_ledger",
    // search / projection
    "chapter_index",
    "entity_index",
    "volume_health_projection",
    "book_health_projection",
    "production_timeline_projection",
    "reader_desire_health_projection",
    "quality_trend_projection",
];

fn table_names(conn: &libsql_rusqlite::Connection) -> HashSet<String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap();
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<HashSet<_>, _>>()
        .unwrap();
    names
}

fn assert_fk_and_quickcheck(conn: &libsql_rusqlite::Connection) {
    let fk_rows: i64 = {
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        let mut rows = stmt.query([]).unwrap();
        let mut n = 0;
        while rows.next().unwrap().is_some() {
            n += 1;
        }
        n
    };
    assert_eq!(fk_rows, 0, "must be foreign-key clean");
    let qc: String = conn
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(qc, "ok");
}

#[test]
fn fresh_project_has_all_21_2_core_tables() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    let actual = table_names(&connection);
    for table in EXPECTED_CORE_TABLES {
        assert!(
            actual.contains(*table),
            "missing core table: {table} (found {actual:?})"
        );
    }
    assert_fk_and_quickcheck(&connection);
    let count = EXPECTED_CORE_TABLES.len();
    // None of the 61 core tables may be absent: the acceptance gate forbids the
    // previous 55-table gap.
    let missing: Vec<_> = EXPECTED_CORE_TABLES
        .iter()
        .filter(|t| !actual.contains(**t))
        .collect();
    assert!(
        missing.is_empty(),
        "still missing {} core tables",
        missing.len()
    );
    // Cross-check the §21.3 index set at least exists.
    for index in [
        "idx_canon_facts_subject_time",
        "idx_canon_relations_subject",
        "idx_dependency_edges_source",
        "idx_chapters_volume_no",
    ] {
        let exists: bool = {
            let mut stmt = connection
                .prepare(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name=?1)",
                )
                .unwrap();
            let v: bool = stmt.query_row([index], |r| r.get(0)).unwrap();
            v
        };
        assert!(exists, "missing index {index}");
    }
    drop(connection);
    // 61 tables are mandated by §21.2; canon_timepoints is added by the Canon
    // data model (§11.1), taking the authoritative set to 62 in this migration.
    assert_eq!(
        count, 62,
        "architecture §21.2 (61) + Canon data model (canon_timepoints)"
    );
}

#[test]
fn old_m0a_database_upgrades_in_place_keeping_hashes() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let dir = paths.project_dir("m0a").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("project.db");

    // Build a v2/v3 (M0A-only) database: hand-apply the two historical SQL
    // files, then seed a committed artifact + command + commit receipt so we can
    // assert every stored hash/digest survives the upgrade untouched.
    let conn = ai_novel_production_system_core::project_db::connection::open_connection(&db, false)
        .unwrap();
    conn.execute_batch(include_str!("../migrations/0002_project_authority.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../migrations/0003_project_command.sql"))
        .unwrap();
    conn.execute(
        "INSERT INTO migration_history(version, applied_at) VALUES (2, ?1), (3, ?1)",
        [NOW],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO project_meta(project_id, title, created_at, updated_at) VALUES ('m0a', 'A', ?1, ?1)",
        [NOW],
    ).unwrap();
    conn.execute(
        "INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at, committed_at) VALUES ('a1', 'm0a', 'FinalChapter', 'CHAPTER', 'k1', 1, 'm0a.artifact.v1', 'COMMITTED', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '{}', ?1, ?1)",
        [NOW],
    ).unwrap();
    // A valid Command record that owns the receipt, so the commit_receipts FK
    // (project_id, command_id) resolves.
    conn.execute(
        "INSERT INTO commands(project_id, command_id, idempotency_key, request_digest, status, created_at) VALUES ('m0a', 'cmd:1', 'idem:1', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'COMMITTED', ?1)",
        [NOW],
    ).unwrap();
    // A frozen CommitReceipt whose receipt_digest and bundle_digest must be
    // preserved byte-for-byte through the migration.
    const RECEIPT_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const BUNDLE_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    conn.execute(
        "INSERT INTO commit_receipts(receipt_id, project_id, commit_id, command_id, idempotency_key, before_revision, after_revision, bundle_digest, receipt_json, receipt_digest, committed_at) VALUES ('rcp:1', 'm0a', 'commit:1', 'cmd:1', 'idem:1', 0, 1, ?1, '{\"receiptId\":\"rcp:1\"}', ?2, ?3)",
        libsql_rusqlite::params![BUNDLE_DIGEST, RECEIPT_DIGEST, NOW],
    ).unwrap();
    drop(conn);

    // Register in the catalog so the manager's open-policy gate can resolve it.
    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    CatalogRepository::register_project(&catalog, "m0a", &dir.to_string_lossy(), "A", NOW).unwrap();
    drop(catalog);

    // Open via the manager: it must detect the missing v4 and migrate in place,
    // creating a backup first (policy in open()), then leave a v4 DB that keeps
    // the original artifact hash + receipt digest intact and passes the gates.
    let manager = ProjectManager::new(paths.clone());
    let connection = manager.open("m0a", "A", NOW).unwrap();
    assert_fk_and_quickcheck(&connection);
    let v4: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM migration_history WHERE version = 4)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(v4, "v4 migration must be recorded after upgrade");
    let hash: String = connection
        .query_row(
            "SELECT payload_hash FROM artifacts WHERE artifact_id = 'a1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        hash, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "existing artifact hash must be untouched by the upgrade"
    );
    let receipt_digest: String = connection
        .query_row(
            "SELECT receipt_digest FROM commit_receipts WHERE receipt_id = 'rcp:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        receipt_digest, RECEIPT_DIGEST,
        "existing commit receipt digest must be untouched by the upgrade"
    );
    let has_longterm: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='canon_facts')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(has_longterm, "long-term tables must exist after upgrade");
    drop(connection);
}

#[test]
fn migration_rolls_back_atomically_on_midway_failure() {
    // §23 / B01-P0: a migration must be transaction-atomic. On a deterministic
    // failure in the middle of the M0B migration, nothing may be half-applied:
    // no v4 marker, no half-built table/index, original M0A data intact. We
    // trigger the failure with a *conflicting schema* — a `canon_facts` VIEW that
    // occupies the name, so the `CREATE TABLE IF NOT EXISTS` is skipped and the
    // later `CREATE INDEX ... ON canon_facts(...)` fails when SQLite refuses to
    // index a view. The whole migrate_project transaction rolls back.
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let dir = paths.project_dir("m0a").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("project.db");
    let conn = ai_novel_production_system_core::project_db::connection::open_connection(&db, false)
        .unwrap();
    conn.execute_batch(include_str!("../migrations/0002_project_authority.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../migrations/0003_project_command.sql"))
        .unwrap();
    conn.execute(
        "INSERT INTO migration_history(version, applied_at) VALUES (2, ?1), (3, ?1)",
        [NOW],
    )
    .unwrap();
    conn.execute("INSERT INTO project_meta(project_id, title, created_at, updated_at) VALUES ('m0a', 'A', ?1, ?1)", [NOW])
        .unwrap();
    conn.execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at, committed_at) VALUES ('a1', 'm0a', 'FinalChapter', 'CHAPTER', 'k1', 1, 'm0a.artifact.v1', 'COMMITTED', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '{}', ?1, ?1)", [NOW])
        .unwrap();
    // Seed the command and its frozen receipt so rollback proves both the
    // artifact hash and receipt digest survive a failed and successful retry.
    conn.execute(
        "INSERT INTO commands(project_id, command_id, idempotency_key, request_digest, status, created_at) VALUES ('m0a', 'cmd:rollback', 'idem:rollback', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'COMMITTED', ?1)",
        [NOW],
    ).unwrap();
    const ROLLBACK_RECEIPT_DIGEST: &str =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    conn.execute(
        "INSERT INTO commit_receipts(receipt_id, project_id, commit_id, command_id, idempotency_key, before_revision, after_revision, bundle_digest, receipt_json, receipt_digest, committed_at) VALUES ('rcp:rollback', 'm0a', 'commit:rollback', 'cmd:rollback', 'idem:rollback', 0, 1, 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', '{\"receiptId\":\"rcp:rollback\"}', ?1, ?2)",
        libsql_rusqlite::params![ROLLBACK_RECEIPT_DIGEST, NOW],
    ).unwrap();
    let before_artifact_hash: String = conn
        .query_row(
            "SELECT payload_hash FROM artifacts WHERE artifact_id = 'a1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let before_receipt_digest: String = conn
        .query_row(
            "SELECT receipt_digest FROM commit_receipts WHERE receipt_id = 'rcp:rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // Conflict marker: a view grabs the canon_facts name before the migration.
    conn.execute_batch("CREATE VIEW canon_facts AS SELECT 'x' AS fake_id;")
        .unwrap();
    drop(conn);

    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    CatalogRepository::register_project(&catalog, "m0a", &dir.to_string_lossy(), "A", NOW).unwrap();
    drop(catalog);

    let manager = ProjectManager::new(paths.clone());
    let opened = manager.open("m0a", "A", NOW);
    assert!(
        opened.is_err(),
        "migration must fail when canon_facts is a conflict schema"
    );
    drop(opened);

    // Re-open read-only to inspect: no half-applied migration may remain.
    let conn = ai_novel_production_system_core::project_db::connection::open_connection(&db, true)
        .unwrap();
    // 1. schema version must NOT be marked upgraded to v4.
    let v4: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM migration_history WHERE version = 4)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!v4, "failed migration must not record v4");
    // 2. no half-built new table after the conflict point (canon_relations is
    //    created in the same migration, after canon_facts) may survive.
    let canon_relations: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='canon_relations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !canon_relations,
        "no half-built table may survive a rollback"
    );
    let canon_fields: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_canon_facts_source_chapter')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!canon_fields, "no half-built index may survive a rollback");
    // 4. original M0A data is intact.
    let artifact_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id = 'm0a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        artifact_count, 1,
        "original artifact must survive a rollback"
    );
    let hash: String = conn
        .query_row(
            "SELECT payload_hash FROM artifacts WHERE artifact_id = 'a1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        hash,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    let title: String = conn
        .query_row(
            "SELECT title FROM project_meta WHERE project_id = 'm0a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(title, "A");
    let receipt_digest: String = conn
        .query_row(
            "SELECT receipt_digest FROM commit_receipts WHERE receipt_id = 'rcp:rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        receipt_digest, before_receipt_digest,
        "receipt digest must survive rollback"
    );
    drop(conn);

    // 7. fix the conflict and re-run: migration must now succeed cleanly.
    let conn = ai_novel_production_system_core::project_db::connection::open_connection(&db, false)
        .unwrap();
    conn.execute_batch("DROP VIEW canon_facts;").unwrap();
    drop(conn);
    let connection = manager.open("m0a", "A", NOW).unwrap();
    assert_fk_and_quickcheck(&connection);
    let v4_after: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM migration_history WHERE version = 4)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        v4_after,
        "after fixing the conflict the migration must succeed"
    );
    let after_artifact_hash: String = connection
        .query_row(
            "SELECT payload_hash FROM artifacts WHERE artifact_id = 'a1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let after_receipt_digest: String = connection
        .query_row(
            "SELECT receipt_digest FROM commit_receipts WHERE receipt_id = 'rcp:rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        after_artifact_hash, before_artifact_hash,
        "artifact hash must survive retry"
    );
    assert_eq!(
        after_receipt_digest, before_receipt_digest,
        "receipt digest must survive retry"
    );
    drop(connection);
}

#[test]
fn duplicate_remaps_all_long_term_project_id_and_resets_runtime() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let connection = manager.create("src", "Src", NOW).unwrap();

    // Seed representative rows across authority, projection, and runtime
    // families so we can assert the copy remaps/clears each respectively.
    connection.execute(
        "INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('bk:1', 'src', 1, 'B', 'ACTIVE', ?1, ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, created_at, updated_at) VALUES ('vol:1', 'src', 'bk:1', 1, 'V', 'PLANNING', ?1, ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, created_at, updated_at) VALUES ('chap:1', 'src', 'vol:1', 1, 'C', 'NORMAL', 'PLANNED', ?1, ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES ('ent:1', 'src', '王五', 'PERSON', ?1, ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, authority_level, created_at, updated_at) VALUES ('fact:1', 'src', 'ent:1', 'is', '{}', 'C', ?1, ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, created_by, created_at) VALUES ('edge:1', 'src', 'src', 'chap:1', 'PLAN_INPUT', 'CHAPTER', 'SOFT', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'test', ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO project_runtime_state(project_id, current_phase, run_state, updated_at) VALUES ('src', 'VOLUME_PRODUCTION', 'ACTIVE', ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO jobs(job_id, project_id, job_type, chapter_id, status, created_at, updated_at) VALUES ('job:1', 'src', 'CHAPTER', 'chap:1', 'RUNNING', ?1, ?1)",
        [NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO model_runs(model_run_id, project_id, provider, model, idempotency_key, capability, status, created_at, updated_at) VALUES ('mr:1', 'src', 'p', 'm', 'mkey-1', 'c', 'COMPLETED', ?1, ?1)",
        [NOW],
    ).unwrap();
    drop(connection);

    manager
        .duplicate("src", "copy", "2026-08-26T00:00:01Z")
        .unwrap();
    let copy_conn = manager.read_connection("copy", NOW).unwrap();

    // Authority rows are remapped to the copy id. Each table's business key
    // column name differs, so pair the table with its id column.
    for (table, id_col, id) in [
        ("books", "book_id", "bk:1"),
        ("volumes", "volume_id", "vol:1"),
        ("chapters", "chapter_id", "chap:1"),
        ("canon_entities", "entity_id", "ent:1"),
        ("canon_facts", "fact_id", "fact:1"),
        ("dependency_edges", "edge_id", "edge:1"),
    ] {
        let pid: String = copy_conn
            .query_row(
                &format!("SELECT project_id FROM {table} WHERE {id_col} = ?1"),
                [id],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| panic!("row {id} in {table} must exist in copy"));
        assert_eq!(
            pid, "copy",
            "{table} row {id} must be remapped to the copy id"
        );
    }

    // Runtime / lease / telemetry rows are cleared (reset), not copied.
    let runtime: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM project_runtime_state WHERE project_id = 'copy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(runtime, 0, "runtime state must be reset on copy");
    let jobs: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM jobs WHERE project_id = 'copy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(jobs, 0, "jobs must not be copied");
    let runs: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id = 'copy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(runs, 0, "model telemetry must not be copied");

    // A copy receipt records the duplication.
    let copies: i64 = copy_conn
        .query_row(
            "SELECT COUNT(*) FROM copy_receipts WHERE target_project_id = 'copy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(copies, 1, "copy must write a CopyReceipt");

    // The copy is FK-clean and self-consistent.
    assert_fk_and_quickcheck(&copy_conn);
    drop(copy_conn);

    // The source keeps its identity and runtime.
    let src_conn = manager.read_connection("src", NOW).unwrap();
    let src_jobs: i64 = src_conn
        .query_row(
            "SELECT COUNT(*) FROM jobs WHERE project_id = 'src'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(src_conn);
    assert_eq!(src_jobs, 1, "source keeps its own runtime rows");
}

#[test]
fn registry_entries_all_point_at_real_tables() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    let actual = table_names(&connection);

    let mut stmt = connection
        .prepare("SELECT authoritative_table FROM authority_registry")
        .unwrap();
    let authority_tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(stmt);
    for table in &authority_tables {
        assert!(
            actual.contains(table.as_str()),
            "authority_registry points at missing table {table}"
        );
    }

    let mut stmt = connection
        .prepare("SELECT table_name FROM table_family_registry")
        .unwrap();
    let family_tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for table in &family_tables {
        assert!(
            actual.contains(table.as_str()),
            "table_family_registry points at missing table {table}"
        );
    }

    // The previously reserved, never-created names must be gone.
    assert!(
        !actual.contains("reader_desire_ledger"),
        "reader_desire_ledger must be corrected"
    );
    assert!(
        !actual.contains("narrative_quality_reports"),
        "narrative_quality_reports must be corrected"
    );
    // The frozen V2.1 names are authoritative now.
    assert!(actual.contains("reader_desires"));
    assert!(actual.contains("quality_trend_projection"));
}

#[test]
fn copy_policy_registry_covers_every_project_id_table() {
    // Altitude guard (B01-T08): every table that carries a `project_id` column
    // must have a copy policy, so the Copy-Project remap can never silently
    // leave a source-bound project_id in a copied long-term table. This makes a
    // future migration that adds a project_id table but forgets its policy a
    // loud test failure rather than a silent copy leak.
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();

    // Collect every table with a project_id column from the live schema.
    let mut table_stmt = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap();
    let names: Vec<String> = table_stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(table_stmt);

    let mut with_project_id = Vec::new();
    for table in &names {
        let mut col_stmt = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let has_project: bool = col_stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .iter()
            .any(|c| c == "project_id");
        if has_project {
            with_project_id.push(table.clone());
        }
    }

    // The explicitly-handled tables (remap_copy rewrites project_meta directly
    // and deletes the M0A audit rows directly; they are intentionally absent
    // from the policy registry).
    let explicit = [
        "project_meta",
        "commands",
        "commit_receipts",
        "gc_receipts",
        "project_command_receipts",
    ];
    let mut unpolicied: Vec<&String> = with_project_id
        .iter()
        .filter(|t| {
            !explicit.contains(&t.as_str())
                && !{
                    let exists: bool = connection
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM copy_policy_registry WHERE table_name = ?1)",
                            [t.as_str()],
                            |row| row.get(0),
                        )
                        .unwrap();
                    exists
                }
        })
        .collect();
    unpolicied.sort();
    assert!(
        unpolicied.is_empty(),
        "tables with project_id but no copy policy (B01-T08 drift): {unpolicied:?}"
    );
}

#[test]
fn repeated_open_does_not_repeat_migration_or_backup_storm() {
    // §23 / B01-P1: reopening an already-upgraded project must not re-run the
    // destructive migration, must not grow the schema version again, and must not
    // produce a migration backup on every open. Only the single upgrade itself
    // writes a migration backup; subsequent opens are no-op.
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let dir = paths.project_dir("m0a").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("project.db");
    let conn = ai_novel_production_system_core::project_db::connection::open_connection(&db, false)
        .unwrap();
    conn.execute_batch(include_str!("../migrations/0002_project_authority.sql"))
        .unwrap();
    conn.execute_batch(include_str!("../migrations/0003_project_command.sql"))
        .unwrap();
    conn.execute(
        "INSERT INTO migration_history(version, applied_at) VALUES (2, ?1), (3, ?1)",
        [NOW],
    )
    .unwrap();
    conn.execute("INSERT INTO project_meta(project_id, title, created_at, updated_at) VALUES ('m0a', 'A', ?1, ?1)", [NOW])
        .unwrap();
    drop(conn);

    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    CatalogRepository::register_project(&catalog, "m0a", &dir.to_string_lossy(), "A", NOW).unwrap();
    drop(catalog);

    let manager = ProjectManager::new(paths.clone());
    // First open performs the migration (one backup), then close.
    drop(manager.open("m0a", "A", NOW).unwrap());
    let backups_after_one = count_migration_backups(&dir);
    assert_eq!(
        backups_after_one, 1,
        "the one upgrade must write exactly one migration backup"
    );

    // Reopen several times: migration must not recur, version must not grow,
    // and no additional migration backup may appear (no storm).
    for _ in 0..3 {
        let conn = manager.open("m0a", "A", NOW).unwrap();
        let max_version: i64 = conn
            .query_row("SELECT MAX(version) FROM migration_history", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            max_version, 17,
            "schema version must settle at CRAFT v3 v17 and not grow"
        );
        drop(conn);
    }
    let backups_after_reopens = count_migration_backups(&dir);
    assert_eq!(
        backups_after_reopens, 1,
        "reopening an already-migrated project must not create a migration backup storm"
    );
    // The DB is intact and up to date.
    let conn = manager.read_connection("m0a", NOW).unwrap();
    assert_fk_and_quickcheck(&conn);
}

fn count_migration_backups(dir: &std::path::Path) -> usize {
    let backups = dir.join("backups");
    if !backups.is_dir() {
        return 0;
    }
    std::fs::read_dir(&backups)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("migration-")
        })
        .count()
}

#[test]
fn stable_id_boundary_roundtrips_across_project_copy() {
    // The id depends only on stable identity material (a frozen parent stable id
    // + an immutable child id), never a rowid, title, or chapter number. A copied
    // project therefore reproduces the same opaque id for the same immutable
    // child, proving the boundary never leaks a mutable display field.
    let parent = stable_id::mint(stable_id::StableIdKind::Volume).unwrap();
    let seed: String = uuid::Uuid::from_bytes([0xab; 16]).to_string(); // immutable hex child seed
    let a = stable_id::derive(stable_id::StableIdKind::Chapter, &parent, &seed).unwrap();
    let b = stable_id::derive(stable_id::StableIdKind::Chapter, &parent, &seed).unwrap();
    assert_eq!(
        a, b,
        "identical stable material must yield the identical id"
    );
    let copied = stable_id::derive(stable_id::StableIdKind::Chapter, &parent, &seed).unwrap();
    assert_eq!(a, copied, "a copy rewrites no stable material → same refs");
    assert!(stable_id::validate_stable_id(&a, stable_id::StableIdKind::Chapter).is_ok());
    assert!(stable_id::validate_stable_id(&a, stable_id::StableIdKind::Volume).is_err());
    // A display-path discriminator (the old `vol:1:chap:1` form) is rejected at
    // the boundary — it is mutable identity material.
    assert!(stable_id::derive(stable_id::StableIdKind::Chapter, &parent, "vol:1:chap:1").is_err());
}

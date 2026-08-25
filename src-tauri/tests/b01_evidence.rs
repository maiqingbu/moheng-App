//! B01 evidence generator (test-time only).
//!
//! Emits the schema inventory and registry-integrity evidence that B01 must
//! return: it creates a fresh project.db and writes `evidence/B01/`
//! schema_inventory.json + registry_integrity.json from the live schema.

use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use serde_json::json;
use std::path::{Path, PathBuf};

fn dump(root: &Path) {
    let manager = ProjectManager::new(AppPaths::new(root.to_path_buf()));
    let connection = manager
        .create("evidence", "A", "2026-08-26T00:00:00Z")
        .unwrap();

    // 1. Tables + indexes inventory.
    let mut table_stmt = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();
    let tables: Vec<String> = table_stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(table_stmt);

    let mut index_stmt = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='index' ORDER BY name")
        .unwrap();
    let indexes: Vec<String> = index_stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(index_stmt);

    // 2. Registry integrity: authority_registry + table_family_registry entries,
    //    checked to resolve to a real table.
    let mut auth_stmt = connection
        .prepare("SELECT authority_key, authoritative_table, write_owner, projection_only FROM authority_registry ORDER BY authority_key")
        .unwrap();
    let authorities: Vec<serde_json::Value> = auth_stmt
        .query_map([], |row| {
            Ok(json!({
                "authority_key": row.get::<_, String>(0)?,
                "authoritative_table": row.get::<_, String>(1)?,
                "write_owner": row.get::<_, String>(2)?,
                "projection_only": row.get::<_, i64>(3)?,
            }))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(auth_stmt);

    let mut fam_stmt = connection
        .prepare("SELECT table_name, family, authority_key, retention_class FROM table_family_registry ORDER BY table_name")
        .unwrap();
    let families: Vec<serde_json::Value> = fam_stmt
        .query_map([], |row| {
            Ok(json!({
                "table_name": row.get::<_, String>(0)?,
                "family": row.get::<_, String>(1)?,
                "authority_key": row.get::<_, String>(2)?,
                "retention_class": row.get::<_, String>(3)?,
            }))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(fam_stmt);

    let missing_auth: Vec<String> = authorities
        .iter()
        .map(|v| v["authoritative_table"].as_str().unwrap().to_string())
        .filter(|t| !tables.contains(t))
        .collect();
    let missing_family: Vec<String> = families
        .iter()
        .map(|v| v["table_name"].as_str().unwrap().to_string())
        .filter(|t| !tables.contains(t))
        .collect();

    // 3. FK + quick_check.
    let fk_rows: i64 = {
        let mut stmt = connection.prepare("PRAGMA foreign_key_check").unwrap();
        let mut rows = stmt.query([]).unwrap();
        let mut n = 0;
        while rows.next().unwrap().is_some() {
            n += 1;
        }
        n
    };
    let quick_check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .unwrap();
    drop(connection);

    let inventory = json!({
        "tables": tables,
        "table_count": tables.len(),
        "indexes": indexes,
        "index_count": indexes.len(),
        "foreign_key_violations": fk_rows,
        "quick_check": quick_check,
    });
    let registry = json!({
        "authority_registry": authorities,
        "table_family_registry": families,
        "missing_authoritative_tables": missing_auth,
        "missing_registered_tables": missing_family,
    });
    // cargo test runs with CWD = src-tauri/, so write relative to the project
    // root (CARGO_MANIFEST_DIR = .../src-tauri) to land in <root>/evidence/B01.
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let evidence_dir = project_root.join("evidence/B01");
    std::fs::create_dir_all(&evidence_dir).unwrap();
    std::fs::write(
        evidence_dir.join("schema_inventory.json"),
        serde_json::to_string_pretty(&inventory).unwrap(),
    )
    .unwrap();
    std::fs::write(
        evidence_dir.join("registry_integrity.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    )
    .unwrap();
    println!(
        "schema_inventory.json + registry_integrity.json written to {:?}",
        evidence_dir
    );
}

#[test]
fn generate_evidence() {
    let temp = tempfile::tempdir().unwrap();
    dump(temp.path());
}

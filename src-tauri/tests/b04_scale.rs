use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::ops::{FixtureSpec, LongBookFixture};
use tempfile::tempdir;

const NOW: &str = "2026-08-27T00:00:00Z";

/// This is deliberately opt-in: it materializes the complete B04 scale
/// contract and is run by the release evidence runner with B04_RUN_SCALE=1.
/// The ordinary regression suite keeps the bounded fixture for developer speed.
#[test]
#[ignore = "release-scale evidence only"]
fn b04_long_book_10m_materializes_the_declared_scale() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B04 scale", NOW).unwrap();
    let spec = FixtureSpec::long_book_10m("b04-release-scale");
    let manifest = LongBookFixture::materialize(&connection, &spec, NOW).unwrap();
    for (table, expected) in [
        ("books", manifest.counts.books),
        ("volumes", manifest.counts.volumes),
        ("chapters", manifest.counts.chapters),
        ("artifacts", manifest.counts.artifacts),
        ("canon_facts", manifest.counts.canon_facts),
        ("dependency_edges", manifest.counts.dependencies),
        ("model_runs", manifest.counts.model_runs),
    ] {
        let actual: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE project_id='p1'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(actual as u64, expected, "scale count mismatch for {table}");
    }
    println!(
        "B04_SCALE_MATERIALIZED books={} volumes={} chapters={} artifacts={} canon_facts={} dependencies={} model_runs={} target_words={} status=PASS",
        manifest.counts.books,
        manifest.counts.volumes,
        manifest.counts.chapters,
        manifest.counts.artifacts,
        manifest.counts.canon_facts,
        manifest.counts.dependencies,
        manifest.counts.model_runs,
        manifest.target_words
    );
}

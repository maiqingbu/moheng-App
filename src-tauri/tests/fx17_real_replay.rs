//! FX-17 offline replay of a real-model chapter run (no network, no cost).
//!
//! MOHENG_REPLAY_ROOT: an app-data copy (catalog.db + projects/<id>/project.db)
//! MOHENG_REPLAY_PROJECT_ID / MOHENG_REPLAY_CHAPTER_NO
//! MOHENG_REPLAY_RESPONSES: dir with <CAPABILITY>.json raw provider bodies
use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const NOW: &str = "2026-09-12T06:00:00Z";

struct ReplayGateway(PathBuf);

impl ProductionModelGateway for ReplayGateway {
    fn invoke(&self, capability: &str, _input: &Value) -> Result<ProductionModelResponse, String> {
        let raw = std::fs::read_to_string(self.0.join(format!("{capability}.json")))
            .map_err(|_| format!("REPLAY_MISSING:{capability}"))?;
        let body: Value = serde_json::from_str(&raw).unwrap();
        let content = body["choices"][0]["message"]["content"].as_str().unwrap();
        Ok(ProductionModelResponse::deterministic(
            serde_json::from_str(content).unwrap(),
        ))
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
#[ignore = "offline replay of recorded real-model responses; needs MOHENG_REPLAY_* env"]
fn replay_recorded_real_chapter_run() {
    let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} required"));
    let temp = tempfile::tempdir().unwrap();
    copy_dir(Path::new(&env("MOHENG_REPLAY_ROOT")), temp.path());
    let project_id = env("MOHENG_REPLAY_PROJECT_ID");
    libsql_rusqlite::Connection::open(temp.path().join("catalog.db"))
        .unwrap()
        .execute(
            "UPDATE projects SET project_path=?2 WHERE project_id=?1",
            (
                &project_id,
                temp.path()
                    .join("projects")
                    .join(&project_id)
                    .to_str()
                    .unwrap(),
            ),
        )
        .unwrap();
    let chapter_no: i64 = env("MOHENG_REPLAY_CHAPTER_NO").parse().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(ReplayGateway(PathBuf::from(env("MOHENG_REPLAY_RESPONSES")))),
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let chapter_id: String = connection
        .query_row(
            "SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no=?2",
            (&project_id, chapter_no),
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    let revision = state.manager.overview(&project_id).unwrap().1;
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:fx17:replay","idempotencyKey":"key:fx17:replay",
            "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,
                "targetChineseCharacters":1800}
        }),
    );
    match result {
        Ok(response) => {
            assert_eq!(response["finalStatus"], "committed", "{response}");
        }
        Err(error) => panic!("replay failed: {error}"),
    }
}

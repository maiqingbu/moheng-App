//! FX-21 real-model check: the configured provider classifies differently
//! worded assistant questions (spends a few model calls).
use ai_novel_production_system_core::commands::{classify_assistant_intent_impl, KernelState};
use ai_novel_production_system_core::production::ConfiguredProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

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
#[ignore = "real provider: a few paid classification calls on a copy of the work"]
fn real_provider_understands_differently_worded_assistant_questions() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_ASSISTANT_INTENT").as_deref(),
        Ok("1")
    );
    let source = std::path::PathBuf::from(std::env::var("MOHENG_REAL_APP_DATA_DIR").unwrap());
    let project_id = std::env::var("MOHENG_REAL_PROJECT_ID").unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::copy(
        source.join("provider-settings.json"),
        temp.path().join("provider-settings.json"),
    )
    .unwrap();
    std::fs::copy(source.join("catalog.db"), temp.path().join("catalog.db")).unwrap();
    copy_dir(
        &source.join("projects").join(&project_id),
        &temp.path().join("projects").join(&project_id),
    );
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
    let paths = AppPaths::new(temp.path());
    let state = KernelState::with_production_gateway(
        paths.clone(),
        Arc::new(ConfiguredProductionGateway::for_desktop(paths)),
    );
    let cases = [
        ("DIAGNOSE", "沈砚清楚姐姐现在在哪吗？", "KNOWLEDGE"),
        (
            "DIAGNOSE",
            "他答应过要找回姐姐，这件事后来有交代吗？",
            "CONTINUITY",
        ),
        (
            "DIAGNOSE",
            "为什么下一章不能直接揭开幕后真凶？",
            "PLANNING_CONSTRAINTS",
        ),
        ("DIAGNOSE", "这本书现在写到哪了？", "PROJECT_OVERVIEW"),
        (
            "CHANGE",
            "让沈砚的左手恢复正常，之前那段代价不算数",
            "RETCON",
        ),
        ("CHANGE", "把第二卷的节奏放慢一些", "CHANGE"),
        ("CHANGE", "姐姐沈青并没有消失，她一直都在", "RETCON"),
        ("REPAIR", "我写过的那段话怎么都搜不出来", "REBUILD_FTS"),
        ("REPAIR", "进度条一直显示零，不对劲", "REBUILD_PROJECTION"),
        (
            "REPAIR",
            "第三章的计划好像是旧的，跟现在的设定对不上",
            "RECOMPILE_CHAPTER_PLAN",
        ),
    ];
    let mut correct = 0;
    for (index, (entry, question, expected)) in cases.iter().enumerate() {
        let resolution = classify_assistant_intent_impl(
            &state,
            json!({
                "projectId":project_id,"question":question,"entry":entry,
                "requestedAt":format!("2026-10-17T14:00:{index:02}Z")
            }),
        )
        .unwrap();
        let ok = resolution["intent"] == *expected;
        correct += usize::from(ok);
        println!("FX21_REAL entry={entry} expected={expected} got={} source={} subjects={} attribute={} ok={ok} question={question}",
            resolution["intent"], resolution["source"], resolution["subjects"], resolution["attribute"]);
        assert_eq!(resolution["source"], "MODEL", "{resolution}");
    }
    println!("FX21_REAL_RESULT correct={correct}/{}", cases.len());
}

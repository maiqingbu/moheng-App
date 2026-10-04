use ai_novel_production_system_core::commands::{self, KernelState};
use ai_novel_production_system_core::export::EXPORT_CHUNK_CHARACTERS;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::export::CurrentFinalChapter;
use libsql_rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::time::Instant;
use tauri::ipc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::WebviewWindowBuilder;
use tempfile::tempdir;

const NOW: &str = "2026-10-03T00:00:00Z";
const PROJECT_ID: &str = "proj-b-r2-04b-export";

fn build_app(root: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(KernelState::new(AppPaths::new(root.to_path_buf())))
        .invoke_handler(commands::handler())
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn build_webview(
    app: &tauri::App<tauri::test::MockRuntime>,
) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
    WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .unwrap()
}

fn invoke(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, String> {
    let request = InvokeRequest {
        cmd: command.into(),
        callback: ipc::CallbackFn(0),
        error: ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_string(),
    };
    let response =
        tauri::test::get_ipc_response(webview, request).map_err(|error| format!("{error:?}"))?;
    response
        .deserialize::<Value>()
        .map_err(|error| format!("{error:?}"))
}

fn seed_project(root: &std::path::Path, chapters: &[(u64, &str, &str)]) {
    let state = KernelState::new(AppPaths::new(root.to_path_buf()));
    let connection = state
        .manager
        .create(PROJECT_ID, "正式导出测试", NOW)
        .unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:export',?1,1,'导出测试','ACTIVE',?2,?2)",
        params![PROJECT_ID, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:export',?1,'book:export',1,'正式卷','CLOSED',1,?2,10000000,?3,?3)",
        params![PROJECT_ID, chapters.iter().map(|item| item.0 as i64).max().unwrap_or(1), NOW],
    ).unwrap();
    for (chapter_number, title, body) in chapters {
        insert_final(&connection, *chapter_number, title, body, 1);
    }
}

fn insert_final(
    connection: &Connection,
    chapter_number: u64,
    title: &str,
    body: &str,
    version: u64,
) {
    let chapter_id = format!("chapter:export:{chapter_number}");
    let artifact_id = format!("final:export:{chapter_number}");
    let scope_key =
        serde_json::to_string(&json!({"level":"CHAPTER","chapterId":chapter_id})).unwrap();
    let payload = json!({
        "payloadSchema":"final-chapter.r1",
        "chapterId":chapter_id,
        "body":body,
        "privateMetadata":"PRIVATE_METADATA_CANARY",
        "internalPrompt":"INTERNAL_PROMPT_CANARY",
        "apiToken":"sk-export-canary-7d2a",
        "harnessRuntime":"HARNESS_PRIVATE_CANARY"
    });
    let payload_hash = sha256_canonical(&payload).unwrap();
    if version == 1 {
        connection.execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES (?1,?2,'volume:export',?3,?4,'COMMITTED',?5,?6,?7,?8,?8)",
            params![chapter_id, PROJECT_ID, chapter_number as i64, title, body.chars().count() as i64, artifact_id, version as i64, NOW],
        ).unwrap();
    } else {
        connection.execute(
            "UPDATE chapters SET final_artifact_id=?1,final_version=?2,word_count=?3 WHERE project_id=?4 AND chapter_id=?5",
            params![artifact_id, version as i64, body.chars().count() as i64, PROJECT_ID, chapter_id],
        ).unwrap();
    }
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'FinalChapter','CHAPTER',?3,?4,'m0a.artifact.v1','COMMITTED','CANON',?5,'{}',?6,?6)",
        params![artifact_id, PROJECT_ID, scope_key, version as i64, payload_hash, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,?2,?3,?4)",
        params![artifact_id, version as i64, payload.to_string(), payload_hash],
    ).unwrap();
    if version == 1 {
        connection.execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'FinalChapter','CHAPTER',?2,?3,?4,?5)",
            params![PROJECT_ID, scope_key, artifact_id, version as i64, payload_hash],
        ).unwrap();
    } else {
        connection.execute(
            "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type='FinalChapter' AND scope_key=?2",
            params![PROJECT_ID, scope_key],
        ).unwrap();
        connection.execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'FinalChapter','CHAPTER',?2,?3,?4,?5)",
            params![PROJECT_ID, scope_key, artifact_id, version as i64, payload_hash],
        ).unwrap();
    }
}

fn insert_candidate(connection: &Connection) {
    let payload =
        json!({"body":"CANDIDATE_BODY_CANARY","privateMetadata":"CANDIDATE_PRIVATE_CANARY"});
    let payload_hash = sha256_canonical(&payload).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('candidate:export','proj-b-r2-04b-export','ChapterCandidate','CHAPTER','chapter:export:2',1,'m0a.artifact.v1','CANDIDATE',?1,'{}',?2)",
        params![payload_hash, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('candidate:export',1,?1,?2)",
        params![payload.to_string(), payload_hash],
    ).unwrap();
}

fn invoke_create_export(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    request_id: &str,
) -> Result<Value, String> {
    invoke(
        webview,
        "create_export",
        json!({"request":{"projectId":PROJECT_ID,"requestId":request_id,"format":"TXT"}}),
    )
}

fn assert_nul_body_is_rejected_without_receipt(body: String, request_id: &str) {
    let root = tempdir().unwrap();
    seed_project(root.path(), &[(1, "NUL边界", &body)]);
    let app = build_app(root.path());
    let webview = build_webview(&app);
    let result = invoke_create_export(&webview, request_id);
    assert!(
        result
            .unwrap_err()
            .contains("EXPORT_SOURCE_NUL_UNSUPPORTED"),
        "NUL data must fail with a typed error before finalization"
    );
    assert!(invoke(
        &webview,
        "query_export_receipt",
        json!({"request":{"projectId":PROJECT_ID,"requestId":request_id}}),
    )
    .is_err());
    let exports = root
        .path()
        .join("projects")
        .join(PROJECT_ID)
        .join("exports");
    assert!(
        fs::read_dir(exports)
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().starts_with("export-")),
        "rejected NUL input must leave no final export directory"
    );
}

#[test]
fn nul_in_chapter_body_is_rejected_without_receipt() {
    assert_nul_body_is_rejected_without_receipt(
        "前缀\0后缀正文".to_string(),
        "export-request-nul-middle",
    );
}

#[test]
fn nul_at_chunk_boundary_is_rejected_without_receipt() {
    let body = format!("{}\0尾声", "甲".repeat(EXPORT_CHUNK_CHARACTERS));
    assert_nul_body_is_rejected_without_receipt(body, "export-request-nul-chunk-boundary");
}

#[test]
fn registered_export_capability_reports_unavailable_without_current_final_chapters() {
    let root = tempdir().unwrap();
    seed_project(root.path(), &[]);
    let app = build_app(root.path());
    let webview = build_webview(&app);
    let capability = invoke(
        &webview,
        "query_export_capability",
        json!({"request":{"projectId":PROJECT_ID}}),
    )
    .unwrap();
    assert_eq!(capability["available"], false);
    assert_eq!(capability["reason"], "NO_CURRENT_FINAL_CHAPTERS");
}

#[test]
fn registered_export_reads_current_final_chapters_and_receipt_survives_app_reopen() {
    let root = tempdir().unwrap();
    seed_project(
        root.path(),
        &[(2, "雨夜", "正式第二章"), (10, "归途", "正式第十章")],
    );
    let state = KernelState::new(AppPaths::new(root.path().to_path_buf()));
    let connection = state.manager.open(PROJECT_ID, "", NOW).unwrap();
    insert_final(&connection, 2, "雨夜", "当前版本第二章", 2);
    insert_candidate(&connection);
    let revision_before: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [PROJECT_ID],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);

    let app = build_app(root.path());
    let webview = build_webview(&app);
    let capability = invoke(
        &webview,
        "query_export_capability",
        json!({"request":{"projectId":PROJECT_ID}}),
    )
    .unwrap();
    assert_eq!(capability["available"], true);
    assert_eq!(capability["currentFinalChapterCount"], 2);
    assert_eq!(capability["supportedFormats"], json!(["TXT"]));

    let receipt = invoke_create_export(&webview, "export-request-e3-e4").unwrap();
    assert_eq!(receipt["status"], "COMPLETED");
    assert_eq!(receipt["chapters"][0]["chapterNumber"], 2);
    assert_eq!(receipt["chapters"][0]["version"], 2);
    assert_eq!(receipt["chapters"][1]["chapterNumber"], 10);
    assert!(receipt["maxChunkCharacters"].as_u64().unwrap() <= 65_536);
    assert!(receipt["maxWorkingBufferBytes"].as_u64().unwrap() <= 65_536 * 4);
    let output_path = receipt["finalPath"].as_str().unwrap();
    let output = fs::read_to_string(output_path).unwrap();
    assert_eq!(
        output,
        "第2章 雨夜\n\n当前版本第二章\n\n第10章 归途\n\n正式第十章\n\n"
    );
    for canary in [
        "PRIVATE_METADATA_CANARY",
        "INTERNAL_PROMPT_CANARY",
        "sk-export-canary-7d2a",
        "HARNESS_PRIVATE_CANARY",
        "CANDIDATE_BODY_CANARY",
        "CANDIDATE_PRIVATE_CANARY",
        "正式第二章",
        "project.db",
    ] {
        assert!(!output.contains(canary), "export leaked {canary}");
    }
    let original = receipt.clone();
    let replay = invoke_create_export(&webview, "export-request-e3-e4").unwrap();
    assert_eq!(replay, original, "retry must return the same receipt");
    let after_export = state.manager.open(PROJECT_ID, "", NOW).unwrap();
    let revision_after: i64 = after_export
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [PROJECT_ID],
            |row| row.get(0),
        )
        .unwrap();
    let current_version: i64 = after_export
        .query_row(
            "SELECT final_version FROM chapters WHERE project_id=?1 AND chapter_id='chapter:export:2'",
            [PROJECT_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revision_after, revision_before);
    assert_eq!(
        current_version, 2,
        "export must not modify FinalChapter state"
    );
    drop(after_export);
    drop(webview);
    drop(app);

    let reopened = build_app(root.path());
    let reopened_webview = build_webview(&reopened);
    let by_request = invoke(
        &reopened_webview,
        "query_export_receipt",
        json!({"request":{"projectId":PROJECT_ID,"requestId":"export-request-e3-e4"}}),
    )
    .unwrap();
    let latest = invoke(
        &reopened_webview,
        "query_latest_export_receipt",
        json!({"request":{"projectId":PROJECT_ID}}),
    )
    .unwrap();
    assert_eq!(by_request, original);
    assert_eq!(latest, original);
}

#[test]
fn registered_export_streams_ten_million_chinese_characters_in_bounded_chunks() {
    let root = tempdir().unwrap();
    let body = "界".repeat(10_000_000);
    seed_project(root.path(), &[(1, "十百万字", &body)]);
    drop(body);

    let app = build_app(root.path());
    let webview = build_webview(&app);
    let started = Instant::now();
    let receipt = invoke_create_export(&webview, "export-request-10m").unwrap();
    let elapsed_ms = started.elapsed().as_millis();
    assert_eq!(receipt["status"], "COMPLETED");
    assert_eq!(receipt["sourceCharacterCount"], 10_000_000);
    assert_eq!(receipt["outputCharacterCount"], 10_000_012);
    assert!(receipt["maxChunkCharacters"].as_u64().unwrap() <= 49_152);
    assert_eq!(receipt["maxWorkingBufferBytes"], 262_144);
    assert_eq!(receipt["chapters"][0]["chapterNumber"], 1);
    assert_eq!(receipt["chapters"][0]["artifactId"], "final:export:1");
    assert_eq!(receipt["chapters"][0]["version"], 1);
    assert_eq!(receipt["chapters"][0]["characterCount"], 10_000_000);

    let mut file = fs::File::open(receipt["finalPath"].as_str().unwrap()).unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 256 * 1024];
    let mut output_bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        output_bytes += read as u64;
        hasher.update(&buffer[..read]);
    }
    assert_eq!(output_bytes, receipt["outputByteCount"].as_u64().unwrap());
    let output_hash = hex::encode(hasher.finalize());
    assert_eq!(output_hash, receipt["sha256"]);
    let mut expected_hasher = Sha256::new();
    expected_hasher.update("第1章 十百万字\n\n".as_bytes());
    let body_chunk = "界".repeat(16_384);
    for _ in 0..(10_000_000 / 16_384) {
        expected_hasher.update(body_chunk.as_bytes());
    }
    for _ in 0..(10_000_000 % 16_384) {
        expected_hasher.update("界".as_bytes());
    }
    expected_hasher.update(b"\n\n");
    assert_eq!(output_hash, hex::encode(expected_hasher.finalize()));
    println!(
        "B_R2_04B_10M_METRICS={}",
        json!({
            "sourceCharacterCount":receipt["sourceCharacterCount"],
            "outputCharacterCount":receipt["outputCharacterCount"],
            "outputByteCount":output_bytes,
            "chapterOrder":receipt["chapters"].as_array().unwrap().iter().map(|chapter| chapter["chapterNumber"].as_u64().unwrap()).collect::<Vec<_>>(),
            "sha256":receipt["sha256"],
            "elapsedMs":elapsed_ms,
            "maxChunkCharacters":receipt["maxChunkCharacters"],
            "maxWorkingBufferBytes":receipt["maxWorkingBufferBytes"],
        })
    );
}

#[test]
fn unsupported_format_fails_closed_without_receipt() {
    let root = tempdir().unwrap();
    seed_project(root.path(), &[(1, "首章", "正式内容")]);
    let app = build_app(root.path());
    let webview = build_webview(&app);
    let result = invoke(
        &webview,
        "create_export",
        json!({"request":{"projectId":PROJECT_ID,"requestId":"export-request-unsupported","format":"EPUB"}}),
    );
    assert!(result.unwrap_err().contains("EXPORT_FORMAT_UNSUPPORTED"));
    let query = invoke(
        &webview,
        "query_export_receipt",
        json!({"request":{"projectId":PROJECT_ID,"requestId":"export-request-unsupported"}}),
    );
    assert!(query.is_err());
}

#[test]
fn credential_canary_in_final_body_blocks_finalize_and_receipt() {
    let root = tempdir().unwrap();
    seed_project(
        root.path(),
        &[(1, "首章", "正文误含 API key: sk-export-canary-7d2a")],
    );
    let app = build_app(root.path());
    let webview = build_webview(&app);
    let request_id = "export-request-secret-canary";
    let result = invoke_create_export(&webview, request_id);
    assert!(result.unwrap_err().contains("EXPORT_SECURITY_SCAN_FAILED"));

    let query = invoke(
        &webview,
        "query_export_receipt",
        json!({"request":{"projectId":PROJECT_ID,"requestId":request_id}}),
    );
    assert!(query.is_err());
    let project_dir = root.path().join("projects").join(PROJECT_ID);
    let export_count = fs::read_dir(project_dir.join("exports"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("export-"))
        .count();
    assert_eq!(export_count, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn r2_runtime_canary_in_final_body_is_rejected_without_export_or_receipt() {
    let mut random = [0_u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .expect("OS random source is available");
    let canary = format!("MOHENG_SECRET_CANARY_{}", hex::encode(random));
    let body = format!("正文误含 Authorization: Bearer {canary}");
    let root = tempdir().unwrap();
    seed_project(root.path(), &[(1, "动态密钥拒绝", &body)]);
    let app = build_app(root.path());
    let webview = build_webview(&app);
    let request_id = "export-request-r2-runtime-canary";
    let result = invoke_create_export(&webview, request_id);
    let error = result.unwrap_err();
    assert!(error.contains("EXPORT_SECURITY_SCAN_FAILED"));
    assert!(!error.contains(&canary));
    let receipt = invoke(
        &webview,
        "query_export_receipt",
        json!({"request":{"projectId":PROJECT_ID,"requestId":request_id}}),
    );
    assert!(receipt.is_err());
    let exports = root
        .path()
        .join("projects")
        .join(PROJECT_ID)
        .join("exports");
    let final_exports = fs::read_dir(&exports)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("export-"))
        .count();
    assert_eq!(final_exports, 0);
    let output_canary_matches = fs::read_dir(exports)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| fs::read(entry.path()).ok())
        .map(|content| {
            content
                .windows(canary.len())
                .filter(|part| *part == canary.as_bytes())
                .count()
        })
        .sum::<usize>();
    println!(
        "R2_F02_NEGATIVE_EXPORT_SCAN={{\"runtimeCanaryGenerated\":true,\"explicitReject\":true,\"receiptExists\":false,\"finalExportCount\":{},\"outputCanaryMatches\":{},\"errorRedacted\":true}}",
        final_exports, output_canary_matches
    );
    assert_eq!(output_canary_matches, 0);
}

#[test]
fn credential_canary_crossing_chunk_boundary_blocks_owner_finalize() {
    let root = tempdir().unwrap();
    let body = format!("{}sk-export-canary-7d2a尾", "界".repeat(49_150));
    seed_project(root.path(), &[(1, "边界安全扫描", &body)]);
    let app = build_app(root.path());
    let webview = build_webview(&app);
    let request_id = "export-request-boundary-secret-canary";
    let result = invoke_create_export(&webview, request_id);
    assert!(result.unwrap_err().contains("EXPORT_SECURITY_SCAN_FAILED"));
    assert!(invoke(
        &webview,
        "query_export_receipt",
        json!({"request":{"projectId":PROJECT_ID,"requestId":request_id}}),
    )
    .is_err());
    let exports = root
        .path()
        .join("projects")
        .join(PROJECT_ID)
        .join("exports");
    assert!(fs::read_dir(exports)
        .unwrap()
        .filter_map(Result::ok)
        .all(|entry| !entry.file_name().to_string_lossy().starts_with("export-")));
}

#[test]
#[ignore = "external harness prepares a synthetic database outside the measured owner process"]
fn r2_prepare_native_memory_probe_fixture() {
    let root = std::path::PathBuf::from(std::env::var("MOHENG_R2_EXPORT_PROBE_ROOT").unwrap());
    let expected_characters: u64 = std::env::var("MOHENG_R2_EXPORT_SQL_PROBE_CHARACTERS")
        .unwrap()
        .parse()
        .unwrap();
    let chapter_count: usize = std::env::var("MOHENG_R2_EXPORT_PROBE_CHAPTERS")
        .unwrap_or_else(|_| "1".into())
        .parse()
        .unwrap();
    assert!(chapter_count > 0);
    let base_characters = expected_characters / chapter_count as u64;
    let remainder = expected_characters % chapter_count as u64;
    let bodies = (0..chapter_count)
        .map(|index| {
            "墨".repeat((base_characters + u64::from((index as u64) < remainder)) as usize)
        })
        .collect::<Vec<_>>();
    let titles = (0..chapter_count)
        .map(|index| format!("内存探针第{}章", index + 1))
        .collect::<Vec<_>>();
    let chapters = bodies
        .iter()
        .zip(&titles)
        .enumerate()
        .map(|(index, (body, title))| (index as u64 + 1, title.as_str(), body.as_str()))
        .collect::<Vec<_>>();
    seed_project(&root, &chapters);
    assert!(root
        .join("projects")
        .join(PROJECT_ID)
        .join("project.db")
        .is_file());
    println!(
        "B_R2_04B_SQL_PROBE_FIXTURE={}",
        json!({
            "sourceCharacters":expected_characters,
            "databasePreparedInSeparateProcess":true,
            "chapterCount":chapter_count,
            "chapterLayout":if chapter_count == 1 { "one current canonical FinalChapter" } else { "multiple current canonical FinalChapters" }
        })
    );
}

#[test]
#[ignore = "external harness measures the formal Export command on a prebuilt synthetic database"]
fn r2_native_export_memory_probe_worker() {
    let root = std::path::PathBuf::from(std::env::var("MOHENG_R2_EXPORT_PROBE_ROOT").unwrap());
    let expected_characters: u64 = std::env::var("MOHENG_R2_EXPORT_SQL_PROBE_CHARACTERS")
        .unwrap()
        .parse()
        .unwrap();
    let app = build_app(&root);
    let webview = build_webview(&app);
    let started = Instant::now();
    let receipt = invoke_create_export(&webview, "r2-native-memory-probe").unwrap();
    let elapsed_ms = started.elapsed().as_millis();
    assert_eq!(receipt["sourceCharacterCount"], expected_characters);
    println!(
        "B_R2_04B_NATIVE_OWNER_MEMORY_PROBE={}",
        json!({
            "sourceCharacters":receipt["sourceCharacterCount"],
            "returnedChunkCharacters":receipt["maxChunkCharacters"],
            "workingBufferBytes":receipt["maxWorkingBufferBytes"],
            "outputBytes":receipt["outputByteCount"],
            "outputSha256":receipt["sha256"],
            "finalPath":receipt["finalPath"],
            "chapters":receipt["chapters"],
            "elapsedMs":elapsed_ms,
            "chapterCount":receipt["chapters"].as_array().map(Vec::len),
            "owner":"registered Tauri handler -> formal Export Runtime -> repository"
        })
    );
}

#[test]
#[ignore = "external harness sets an OS file-size limit to fail a formal export mid-write"]
fn r2_mid_write_failure_cleans_owner_staging_and_receipt() {
    let root = std::path::PathBuf::from(std::env::var("MOHENG_R2_EXPORT_PROBE_ROOT").unwrap());
    let app = build_app(&root);
    let webview = build_webview(&app);
    let request_id = "r2-mid-write-failure";
    let error = invoke_create_export(&webview, request_id).unwrap_err();
    assert!(
        error.contains("EXPORT_IO_ERROR"),
        "unexpected owner error: {error}"
    );
    let exports = root.join("projects").join(PROJECT_ID).join("exports");
    let names = fs::read_dir(&exports)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(names.iter().all(|name| !name.starts_with("export-")));
    assert!(names.iter().all(|name| !name.starts_with(".staging-")));
    assert!(names.iter().all(|name| !name.starts_with(".progress-")));
    assert!(invoke(
        &webview,
        "query_export_receipt",
        json!({"request":{"projectId":PROJECT_ID,"requestId":request_id}}),
    )
    .is_err());
    println!("B_R2_04B_OWNER_MID_WRITE_FAILURE={{\"status\":\"EXPECTED_EXPORT_IO_ERROR_CLEANED\",\"remainingExportsEntries\":{}}}", names.len());
}

fn payload_connection(body: &str) -> (Connection, CurrentFinalChapter) {
    let payload =
        json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:stream","body":body});
    let payload_json = payload.to_string();
    payload_connection_with_json(body, &payload_json)
}

fn payload_connection_with_json(
    body: &str,
    payload_json: &str,
) -> (Connection, CurrentFinalChapter) {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(
        "CREATE TABLE chapters(chapter_id TEXT,project_id TEXT,status TEXT,final_artifact_id TEXT,final_version INTEGER); CREATE TABLE artifacts(artifact_id TEXT,project_id TEXT,artifact_type TEXT,artifact_status TEXT,scope_level TEXT,scope_key TEXT,version INTEGER,payload_hash TEXT); CREATE TABLE artifact_payloads(artifact_id TEXT,version INTEGER,payload_hash TEXT,payload_json TEXT); CREATE TABLE canonical_heads(project_id TEXT,artifact_type TEXT,scope_level TEXT,scope_key TEXT,artifact_id TEXT,version INTEGER,payload_hash TEXT);",
    ).unwrap();
    let scope_key = r#"{"chapterId":"chapter:stream","level":"CHAPTER"}"#;
    let payload =
        json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:stream","body":body});
    let payload_hash = sha256_canonical(&payload).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,status,final_artifact_id,final_version) VALUES ('chapter:stream','project:stream','COMMITTED','final:stream',1)",
        [],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,artifact_status,scope_level,scope_key,version,payload_hash) VALUES ('final:stream','project:stream','FinalChapter','COMMITTED','CHAPTER',?1,1,?2)",
        params![scope_key, payload_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_hash,payload_json) VALUES ('final:stream',1,?1,?2)",
        params![payload_hash, payload_json],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('project:stream','FinalChapter','CHAPTER',?1,'final:stream',1,?2)",
        params![scope_key, payload_hash],
    ).unwrap();
    let payload_rowid = connection
        .query_row(
            "SELECT rowid FROM artifact_payloads WHERE artifact_id='final:stream' AND version=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let chapter = CurrentFinalChapter {
        chapter_id: "chapter:stream".into(),
        chapter_number: 1,
        title: "Stream".into(),
        artifact_id: "final:stream".into(),
        version: 1,
        payload_hash,
        payload_rowid,
        character_count: body.chars().count() as u64,
    };
    (connection, chapter)
}

#[test]
fn repository_streams_json_text_in_bounded_unicode_chunks_without_loss() {
    let body = format!("{}🚀尾\n终\"\\\t\r", "字".repeat(49_151));
    let (connection, chapter) = payload_connection(&body);
    let mut output = String::new();
    let mut largest_chunk = 0;
    let characters =
        ai_novel_production_system_core::repositories::export::stream_final_chapter_body(
            &connection,
            "project:stream",
            &chapter,
            49_152,
            |chunk| {
                let chunk_characters = chunk.chars().count();
                assert!(chunk_characters <= 49_152);
                largest_chunk = largest_chunk.max(chunk_characters);
                output.push_str(chunk);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(characters, body.chars().count() as u64);
    assert_eq!(output, body);
    assert_eq!(largest_chunk, 49_152);
}

#[test]
fn repository_decodes_json_surrogate_pair_without_changing_source_characters() {
    let body = "前缀🚀后缀";
    let raw_payload = r#"{"payloadSchema":"final-chapter.r1","chapterId":"chapter:stream","body":"前缀\uD83D\uDE80后缀"}"#;
    let (connection, chapter) = payload_connection_with_json(body, raw_payload);
    let mut output = String::new();
    let characters =
        ai_novel_production_system_core::repositories::export::stream_final_chapter_body(
            &connection,
            "project:stream",
            &chapter,
            49_152,
            |chunk| {
                output.push_str(chunk);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(output, body);
    assert_eq!(characters, body.chars().count() as u64);
}

#[test]
fn repository_rejects_nul_while_streaming_before_owner_can_finalize() {
    let (connection, chapter) = payload_connection("前缀\0后缀");
    let mut output = String::new();
    let result = ai_novel_production_system_core::repositories::export::stream_final_chapter_body(
        &connection,
        "project:stream",
        &chapter,
        49_152,
        |chunk| {
            output.push_str(chunk);
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err(), "EXPORT_SOURCE_NUL_UNSUPPORTED");
    assert!(output.is_empty());
}

#[test]
fn repository_typed_rejects_raw_nul_without_emitting_partial_body() {
    let body = "前缀\0后缀";
    let raw_payload = "{\"payloadSchema\":\"final-chapter.r1\",\"chapterId\":\"chapter:stream\",\"body\":\"前缀\0后缀\"}";
    let (connection, chapter) = payload_connection_with_json(body, raw_payload);
    let mut output = String::new();
    let result = ai_novel_production_system_core::repositories::export::stream_final_chapter_body(
        &connection,
        "project:stream",
        &chapter,
        49_152,
        |chunk| {
            output.push_str(chunk);
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err(), "EXPORT_SOURCE_NUL_UNSUPPORTED");
    assert!(output.is_empty());
}

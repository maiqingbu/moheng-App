use ai_novel_production_system_core::commands::{query_production_center_impl, KernelState};
use ai_novel_production_system_core::production::{
    ConfiguredProductionGateway, ProductionModelGateway, UnavailableProductionGateway,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository;
use ai_novel_production_system_core::runtime::{
    DurableModelRunner, FakeModelTransport, ModelRunSpec, TransportFailure,
};
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::json;
use std::sync::Arc;

const NOW: &str = "2026-09-10T12:00:00Z";

fn failed_job_fixture(connection: &Connection, project_id: &str) -> String {
    let job_id = "job:xiaomi:r01";
    connection
        .execute(
            "INSERT INTO jobs(job_id,project_id,job_type,chapter_id,status,current_stage,error_code,created_at,updated_at) VALUES (?1,?2,'CHAPTER','chapter:xiaomi:1','FAILED','RENDER','MODEL_TRANSPORT_FAILED',?3,?3)",
            libsql_rusqlite::params![job_id, project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO project_runtime_state(project_id,run_state,updated_at) VALUES (?1,'ACTIVE',?2) ON CONFLICT(project_id) DO UPDATE SET run_state='ACTIVE',updated_at=excluded.updated_at",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO model_runs(model_run_id,project_id,job_id,provider,model,idempotency_key,capability,status,created_at,updated_at) VALUES ('mr:xiaomi:old',?1,?2,'openai-compatible','gpt-4o-mini','key:xiaomi:old','RENDER_CHAPTER','FAILED',?3,?3)",
            libsql_rusqlite::params![project_id, job_id, NOW],
        )
        .unwrap();
    job_id.into()
}

#[test]
fn xiaomi_r01_failed_projection_is_not_active_and_timeline_is_failed() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let connection = manager.create("xiaomi-r01", "Xiaomi R01", NOW).unwrap();
    failed_job_fixture(&connection, "xiaomi-r01");
    drop(connection);
    let state = KernelState::with_production_gateway(paths, Arc::new(UnavailableProductionGateway));

    let projection =
        query_production_center_impl(&state, json!({"projectId":"xiaomi-r01"})).unwrap();
    assert_eq!(projection["jobStatus"], "FAILED");
    assert_eq!(projection["isActive"], false);
    assert_eq!(projection["lastStage"], "RENDER");
    assert_eq!(projection["canRetry"], true);
    assert_eq!(
        projection["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["title"] == "RENDER")
            .unwrap()["status"],
        "failed"
    );
    assert_eq!(projection["modelRuns"][0]["model"], "gpt-4o-mini");
}

#[test]
fn xiaomi_r02_transport_failure_reconciles_runtime_projection() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("xiaomi-r02", "Xiaomi R02", NOW).unwrap();
    let job_id = "job:xiaomi:r02";
    connection
        .execute(
            "INSERT INTO jobs(job_id,project_id,job_type,status,current_stage,created_at,updated_at) VALUES (?1,'xiaomi-r02','CHAPTER','RUNNING','RENDER',?2,?2)",
            libsql_rusqlite::params![job_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO project_runtime_state(project_id,run_state,updated_at) VALUES ('xiaomi-r02','ACTIVE',?1)",
            [NOW],
        )
        .unwrap();
    let spec = ModelRunSpec {
        model_run_id: StableIdentityFactory::from_command(StableIdKind::ModelRun, "xiaomi:r02")
            .unwrap(),
        job_id: Some(job_id.into()),
        step_id: None,
        provider: "openai-compatible".into(),
        model: "mimo-v2.5-pro".into(),
        idempotency_key: "key:xiaomi:r02".into(),
        capability: "RENDER_CHAPTER".into(),
        prompt_ref: None,
        context_manifest_ref: None,
        budget_id: None,
        predicted_cost_cents: 1,
        attempt_no: 1,
        repair_round: 0,
    };
    let mut transport = FakeModelTransport::failure(TransportFailure::Unavailable);
    assert!(DurableModelRunner::prepare(&connection, &spec, NOW).is_ok());
    assert!(DurableModelRunner::execute(&connection, &mut transport, &spec, NOW).is_err());
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0)
            )
            .unwrap(),
        "FAILED"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>("SELECT status FROM jobs WHERE job_id=?1", [job_id], |row| {
                row.get(0)
            })
            .unwrap(),
        "FAILED"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT run_state FROM project_runtime_state WHERE project_id='xiaomi-r02'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "FAILED"
    );
}

#[test]
fn xiaomi_r03_historical_model_run_is_immutable() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("xiaomi-r03", "Xiaomi R03", NOW).unwrap();
    failed_job_fixture(&connection, "xiaomi-r03");
    connection
        .execute(
            "UPDATE project_runtime_state SET run_state='FAILED' WHERE project_id='xiaomi-r03'",
            [],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT model FROM model_runs WHERE model_run_id='mr:xiaomi:old'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "gpt-4o-mini"
    );
}

#[test]
fn xiaomi_r04_new_profile_binds_current_model() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    ProviderSettingsRepository::write_global(
        &paths,
        &json!({
            "providerId":"openai-compatible",
            "modelId":"mimo-v2.5-pro",
            "provider":"OpenAI Compatible",
            "model":"mimo-v2.5-pro",
            "endpoint":"https://api.xiaomimimo.com/v1",
            "enabled":true
        }),
    )
    .unwrap();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let profile = gateway.resolve_profile().unwrap();
    assert_eq!(profile.provider, "openai-compatible");
    assert_eq!(profile.model, "mimo-v2.5-pro");
    assert_eq!(profile.endpoint, "https://api.xiaomimimo.com/v1");
}

#[test]
fn xiaomi_r05_invalid_profile_fails_closed_without_default_model() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    ProviderSettingsRepository::write_global(
        &paths,
        &json!({
            "providerId":"openai-compatible",
            "endpoint":"https://api.xiaomimimo.com/v1",
            "enabled":true
        }),
    )
    .unwrap();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let error = gateway.resolve_profile().unwrap_err();
    assert!(error.contains("MODEL_CONFIG_MISSING:model"));
    assert!(!error.contains("gpt-4o-mini"));
}

#[test]
fn xiaomi_r08_reload_keeps_failed_job_truthful() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let connection = manager.create("xiaomi-r08", "Xiaomi R08", NOW).unwrap();
    failed_job_fixture(&connection, "xiaomi-r08");
    drop(connection);
    drop(manager);
    let state = KernelState::with_production_gateway(paths, Arc::new(UnavailableProductionGateway));
    let projection =
        query_production_center_impl(&state, json!({"projectId":"xiaomi-r08"})).unwrap();
    assert_eq!(projection["jobStatus"], "FAILED");
    assert_eq!(projection["isActive"], false);
    assert_eq!(projection["activeWork"]["label"], "FAILED");
}

#[test]
fn xiaomi_r10_provider_secrets_never_enter_settings_json() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    ProviderSettingsRepository::write_global(
        &paths,
        &json!({
            "providerId":"openai-compatible",
            "modelId":"mimo-v2.5-pro",
            "endpoint":"https://api.xiaomimimo.com/v1",
            "enabled":true,
            "apiKey":"ephemeral-secret-must-not-persist"
        }),
    )
    .unwrap();
    let raw = std::fs::read_to_string(paths.root().join("provider-settings.json")).unwrap();
    assert!(!raw.contains("ephemeral-secret-must-not-persist"));
    assert!(!raw.contains("apiKey"));
}

use super::*;
use libsql_rusqlite::OptionalExtension;
use std::sync::Mutex;

/// Behaves like a configured provider for the length rules. Scene estimates
/// come from `scene_estimates`; render N (0-based) returns `body_lengths[N]`
/// characters (the last entry repeats).
struct LengthGateway {
    scene_estimates: Vec<i64>,
    body_lengths: Vec<usize>,
    inputs: Mutex<Vec<(String, Value)>>,
}

impl LengthGateway {
    fn new(scene_estimates: &[i64], body_lengths: &[usize]) -> Self {
        Self {
            scene_estimates: scene_estimates.to_vec(),
            body_lengths: body_lengths.to_vec(),
            inputs: Mutex::new(Vec::new()),
        }
    }

    fn inputs(&self, capability: &str) -> Vec<Value> {
        self.inputs
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == capability)
            .map(|(_, input)| input.clone())
            .collect()
    }
}

impl ProductionModelGateway for LengthGateway {
    fn enforces_chapter_length(&self) -> bool {
        true
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let renders = self.inputs("RENDER_CHAPTER").len();
        self.inputs
            .lock()
            .unwrap()
            .push((capability.to_string(), input.clone()));
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "PLAN_STRATEGY" {
            response.output["sceneExecution"] = json!(self
                .scene_estimates
                .iter()
                .map(|estimate| json!({"objective":"推进场景","estimatedCharacters":estimate}))
                .collect::<Vec<_>>());
        }
        if capability == "RENDER_CHAPTER" {
            let length = *self
                .body_lengths
                .get(renders)
                .unwrap_or_else(|| self.body_lengths.last().unwrap());
            let body = response.output["body"].as_str().unwrap().to_string();
            let filler = length.saturating_sub(body.chars().count() + 1);
            response.output["body"] = json!(format!("{body}\n{}", "雨".repeat(filler)));
        }
        Ok(response)
    }
}

fn start(
    gateway: Arc<LengthGateway>,
    project_id: &str,
    payload_extra: Value,
    work_range: Option<Value>,
) -> (
    tempfile::TempDir,
    KernelState,
    String,
    Result<Value, String>,
) {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    let connection = state
        .manager
        .create(project_id, "FX18 length", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    if let Some(range) = work_range {
        ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository::write_project(
            &connection,
            project_id,
            &json!({"chapterLengthRange":range}),
            "cmd:fx18:settings",
            NOW,
        )
        .unwrap();
    }
    drop(connection);
    let mut payload = json!({"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]});
    for (key, value) in payload_extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":format!("cmd:{project_id}"),
            "idempotencyKey":format!("key:{project_id}"),"projectId":project_id,
            "expectedRevision":0,"requestedAt":NOW,"payload":payload
        }),
    );
    (temp, state, chapter_id, result)
}

fn final_word_count(state: &KernelState, project_id: &str, chapter_id: &str) -> Option<i64> {
    let connection = state.manager.open(project_id, "FX18 length", NOW).unwrap();
    connection
        .query_row(
            "SELECT CAST(json_extract(p.payload_json,'$.wordCount') AS INTEGER) FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 AND c.chapter_id=?2",
            [project_id, chapter_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

#[test]
fn fx18_body_outside_the_range_is_rewritten_until_it_fits() {
    let gateway = Arc::new(LengthGateway::new(&[800, 900, 700], &[900, 3500, 2600]));
    let (_temp, state, chapter_id, result) =
        start(gateway.clone(), "p-fx18-rewritten", json!({}), None);
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 3);
    assert_eq!(
        renders[0]["chapterLengthRange"],
        json!({"min":2000,"max":3000})
    );
    assert_eq!(renders[1]["previousCharacterCount"], 900);
    assert_eq!(renders[2]["previousCharacterCount"], 3500);
    assert_eq!(
        final_word_count(&state, "p-fx18-rewritten", &chapter_id),
        Some(2600)
    );
}

#[test]
fn fx18_body_still_outside_the_range_after_two_rewrites_is_not_committed() {
    let gateway = Arc::new(LengthGateway::new(&[800, 900, 700], &[600]));
    let (_temp, state, chapter_id, result) =
        start(gateway.clone(), "p-fx18-still-short", json!({}), None);
    let error = result.unwrap_err();
    assert!(
        error.starts_with("B08_RENDER_LENGTH_OUT_OF_RANGE:600:2000-3000"),
        "{error}"
    );
    assert_eq!(gateway.inputs("RENDER_CHAPTER").len(), 3);
    assert_eq!(gateway.inputs("AUDIT_EXTRACT").len(), 0);
    assert_eq!(
        final_word_count(&state, "p-fx18-still-short", &chapter_id),
        None
    );
}

#[test]
fn fx18_plan_sized_below_the_range_is_rejected_before_writing_or_committing_it() {
    let gateway = Arc::new(LengthGateway::new(&[250, 250], &[2500]));
    let (_temp, state, chapter_id, result) =
        start(gateway.clone(), "p-fx18-thin-plan", json!({}), None);
    assert_eq!(result.unwrap_err(), "B08_PLAN_LENGTH_OUT_OF_RANGE");
    assert_eq!(gateway.inputs("RENDER_CHAPTER").len(), 0);
    assert_eq!(
        gateway.inputs("PLAN_STRATEGY")[0]["chapterLengthRange"],
        json!({"min":2000,"max":3000})
    );
    let connection = state
        .manager
        .open("p-fx18-thin-plan", "FX18 length", NOW)
        .unwrap();
    assert!(crate_head_missing(
        &connection,
        "p-fx18-thin-plan",
        &chapter_id
    ));
}

fn crate_head_missing(connection: &Connection, project_id: &str, chapter_id: &str) -> bool {
    ai_novel_production_system_core::repositories::narrative::NarrativeRepository::canonical_head_ref(
        connection,
        project_id,
        "ChapterPlan",
        chapter_id,
    )
    .unwrap()
    .is_none()
}

#[test]
fn fx18_work_range_applies_and_a_chapter_range_overrides_it() {
    let gateway = Arc::new(LengthGateway::new(&[1200, 1300, 1000], &[3600]));
    let (_temp, _state, _chapter_id, result) = start(
        gateway.clone(),
        "p-fx18-work-range",
        json!({}),
        Some(json!({"min":3000,"max":4000})),
    );
    result.unwrap();
    assert_eq!(
        gateway.inputs("RENDER_CHAPTER")[0]["chapterLengthRange"],
        json!({"min":3000,"max":4000})
    );

    let gateway = Arc::new(LengthGateway::new(&[1500, 1500, 1500], &[4500]));
    let (_temp, _state, _chapter_id, result) = start(
        gateway.clone(),
        "p-fx18-chapter-range",
        json!({"chapterLengthRange":{"min":4000,"max":5000}}),
        Some(json!({"min":3000,"max":4000})),
    );
    result.unwrap();
    assert_eq!(
        gateway.inputs("PLAN_STRATEGY")[0]["chapterLengthRange"],
        json!({"min":4000,"max":5000})
    );
}

#[test]
fn fx18_invalid_work_range_is_refused_and_not_stored() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let connection = state
        .manager
        .create("p-fx18-invalid", "FX18 length", NOW)
        .unwrap();
    let error = ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository::write_project(
        &connection,
        "p-fx18-invalid",
        &json!({"chapterLengthRange":{"min":3000,"max":2000}}),
        "cmd:fx18:invalid",
        NOW,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "CHAPTER_LENGTH_RANGE_INVALID");
    let stored = ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository::read_project(
        &connection,
        "p-fx18-invalid",
    )
    .unwrap();
    assert!(stored.get("chapterLengthRange").is_none());
}

#[test]
fn fx18_long_form_plan_compile_is_sized_for_the_chapter_range() {
    let compile = |estimates: &[i64], project_id: &str| {
        let temp = tempfile::tempdir().unwrap();
        let gateway = Arc::new(LengthGateway::new(estimates, &[2500]));
        let state =
            KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
        let connection = state.manager.create(project_id, "FX18 long", NOW).unwrap();
        let chapter_id = seed_chapter_and_gate_long_form(&connection, project_id);
        let volume_id =
            StableIdentityFactory::from_command(StableIdKind::Volume, "b08-volume").unwrap();
        connection
            .execute(
                "UPDATE volumes SET chapter_end=4 WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
            )
            .unwrap();
        drop(connection);
        let result = execute_command_impl(
            &state,
            json!({
                "type":"planning_compile_chapter_plan","commandId":"cmd:fx18:planning","idempotencyKey":"key:fx18:planning",
                "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
                "payload":{"chapterId":chapter_id,"chapterLengthRange":{"min":3000,"max":4000},"scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":[],"reservationRefs":[],"entityRefs":["entity:actual:linzhou"]}}
            }),
        );
        let range = gateway.inputs("PLAN_STRATEGY")[0]["chapterLengthRange"].clone();
        let connection = state.manager.open(project_id, "FX18 long", NOW).unwrap();
        let head_missing = crate_head_missing(&connection, project_id, &chapter_id);
        (result, range, head_missing, temp)
    };
    let (result, range, head_missing, _temp) = compile(&[1200, 1300, 1000], "p-fx18-long-ok");
    assert_eq!(result.unwrap()["finalStatus"], "committed");
    assert_eq!(range, json!({"min":3000,"max":4000}));
    assert!(!head_missing);
    let (result, _, head_missing, _temp) = compile(&[800, 900, 700], "p-fx18-long-thin");
    assert_eq!(result.unwrap_err(), "B08_PLAN_LENGTH_OUT_OF_RANGE");
    assert!(head_missing);
}

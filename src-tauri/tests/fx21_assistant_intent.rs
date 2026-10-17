//! FX-21: the project assistant understands questions with the configured
//! model and falls back to keyword routing.
use ai_novel_production_system_core::commands::{
    classify_assistant_intent_impl, query_assistant_repair_plan_impl, support_session_impl,
    KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::sync::Arc;

const NOW: &str = "2026-10-17T00:00:00Z";

/// Classifies like a configured provider: `reply` is the model's output, or
/// an error when None.
struct IntentGateway {
    reply: Option<Value>,
}

impl ProductionModelGateway for IntentGateway {
    fn classifies_assistant_intent(&self) -> bool {
        true
    }

    fn invoke(&self, capability: &str, _input: &Value) -> Result<ProductionModelResponse, String> {
        assert_eq!(capability, "CLASSIFY_ASSISTANT_INTENT");
        self.reply
            .clone()
            .map(ProductionModelResponse::deterministic)
            .ok_or_else(|| "PROVIDER_UNAVAILABLE".to_string())
    }
}

fn setup(reply: Option<Value>) -> (tempfile::TempDir, KernelState) {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(IntentGateway { reply }),
    );
    drop(state.manager.create("p1", "FX21", NOW).unwrap());
    (temp, state)
}

fn capabilities(session: &Value) -> Vec<String> {
    session["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["capabilityId"].as_str().unwrap().to_string())
        .collect()
}

fn classify_runs(state: &KernelState) -> Vec<String> {
    let connection = state.manager.open("p1", "", NOW).unwrap();
    let mut statement = connection
        .prepare("SELECT status FROM model_runs WHERE capability='CLASSIFY_ASSISTANT_INTENT'")
        .unwrap();
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap()
}

#[test]
fn fx21_diagnose_question_without_keywords_is_routed_by_the_model() {
    let (_temp, state) = setup(Some(
        json!({"intent":"KNOWLEDGE","subjects":["沈砚"],"attribute":"知情"}),
    ));
    let session = support_session_impl(
        &state,
        json!({"projectId":"p1","question":"沈砚清楚姐姐在哪吗？","mode":"DIAGNOSE","chapterId":"chapter:1","entityIds":[],"references":[],"requestedAt":NOW}),
    )
    .unwrap();
    assert_eq!(session["intent"]["source"], "MODEL");
    assert_eq!(session["intent"]["subjects"], json!(["沈砚"]));
    assert_eq!(
        capabilities(&session),
        ["query_knowledge", "query_events", "query_evidence"]
    );
    assert_eq!(classify_runs(&state), ["COMPLETED"]);
}

#[test]
fn fx21_invalid_or_failed_model_reply_falls_back_to_keywords() {
    for reply in [Some(json!({"intent":"DROP_TABLES"})), None] {
        let (_temp, state) = setup(reply);
        let session = support_session_impl(
            &state,
            json!({"projectId":"p1","question":"A 何时知道 B 的身份","mode":"DIAGNOSE","chapterId":"chapter:1","entityIds":[],"references":[],"requestedAt":NOW}),
        )
        .unwrap();
        assert_eq!(session["intent"]["source"], "KEYWORD");
        assert!(session["intent"]["fallbackReason"].is_string());
        assert_eq!(
            capabilities(&session),
            ["query_knowledge", "query_events", "query_evidence"]
        );
    }
}

#[test]
fn fx21_repair_and_change_entries_use_the_model_intent() {
    let (_temp, state) = setup(Some(
        json!({"intent":"REBUILD_FTS","subjects":[],"attribute":null}),
    ));
    let plan = query_assistant_repair_plan_impl(
        &state,
        json!({"projectId":"p1","question":"我写过的那段话怎么都找不到了","requestedAt":NOW}),
    )
    .unwrap();
    assert_eq!(plan["intent"]["intent"], "REBUILD_FTS");
    assert_eq!(plan["intent"]["source"], "MODEL");

    let (_temp, state) = setup(Some(
        json!({"intent":"RETCON","subjects":["林舟"],"attribute":"生死"}),
    ));
    let resolution = classify_assistant_intent_impl(
        &state,
        json!({"projectId":"p1","question":"让林舟活下来","entry":"CHANGE","requestedAt":NOW}),
    )
    .unwrap();
    assert_eq!(resolution["intent"], "RETCON");
    assert_eq!(resolution["attribute"], "生死");
    assert_eq!(
        classify_assistant_intent_impl(
            &state,
            json!({"projectId":"p1","question":"x","entry":"DELETE","requestedAt":NOW}),
        )
        .unwrap_err(),
        "IPC_INVALID:entry"
    );
}

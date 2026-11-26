//! Owner decision 2026-11-27: the assistant plans its queries with the
//! configured model, queries once more when they come back empty, and
//! answers only from what they returned.
use ai_novel_production_system_core::commands::{support_session_impl, KernelState};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-11-27T00:00:00Z";

/// A configured provider: plans foreshadowing first, nothing on the
/// follow-up round, and answers with a sentence citing `cited`.
struct AnswerGateway {
    cited: &'static str,
    calls: Mutex<Vec<(String, Value)>>,
}

impl ProductionModelGateway for AnswerGateway {
    fn classifies_assistant_intent(&self) -> bool {
        true
    }

    fn plans_assistant_answers(&self) -> bool {
        true
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        self.calls
            .lock()
            .unwrap()
            .push((capability.to_string(), input.clone()));
        let reply = match capability {
            "CLASSIFY_ASSISTANT_INTENT" => {
                json!({"intent":"CONTINUITY","subjects":[],"attribute":null})
            }
            "PLAN_ASSISTANT_QUERY" if input.get("earlierResults").is_none() => json!({"steps":[
                {"capabilityId":"query_foreshadows","input":{}},
                {"capabilityId":"run_sql","input":{"sql":"DROP TABLE chapters"}}
            ]}),
            "PLAN_ASSISTANT_QUERY" => json!({"steps":[]}),
            "COMPOSE_ASSISTANT_ANSWER" => json!({"sentences":[
                {"text":"玉符那条伏笔第 8 章前要回收。","sources":[self.cited]}
            ]}),
            other => return Err(format!("unexpected {other}")),
        };
        Ok(ProductionModelResponse::deterministic(reply))
    }
}

fn ask(cited: &'static str) -> (Value, Vec<(String, Value)>) {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(AnswerGateway {
        cited,
        calls: Mutex::new(Vec::new()),
    });
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    drop(state.manager.create("p1", "助手", NOW).unwrap());
    let session = support_session_impl(
        &state,
        json!({"projectId":"p1","question":"还有哪些伏笔没回收？","mode":"DIAGNOSE","entityIds":[],"references":[],"requestedAt":NOW}),
    )
    .unwrap();
    let calls = gateway.calls.lock().unwrap().clone();
    (session, calls)
}

#[test]
fn round130_the_model_plans_only_menu_queries_and_an_empty_result_gets_one_follow_up() {
    let (session, calls) = ask("call:step:1");
    let steps = session["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["capabilityId"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(steps, ["query_foreshadows"]);
    let planner_rounds = calls
        .iter()
        .filter(|(capability, _)| capability == "PLAN_ASSISTANT_QUERY")
        .count();
    assert_eq!(planner_rounds, 2);
    assert_eq!(session["answer"]["planner"], "MODEL");
    // The model planned the queries, so the question's kind is not asked
    // separately, and nothing came back, so no answer is asked for either.
    for skipped in ["CLASSIFY_ASSISTANT_INTENT", "COMPOSE_ASSISTANT_ANSWER"] {
        assert!(
            calls.iter().all(|(capability, _)| capability != skipped),
            "{skipped}"
        );
    }
}

#[test]
fn round130_an_answer_resting_on_nothing_returned_says_so_instead_of_repeating_the_question() {
    // The only query came back empty, so its call cannot be cited, and a
    // made-up record cannot either.
    for cited in ["call:step:1", "fact:made-up"] {
        let (session, _) = ask(cited);
        assert_eq!(
            session["finding"]["supportStatus"], "INSUFFICIENT",
            "{cited}"
        );
        assert_eq!(
            session["finding"]["claim"],
            "现有作品记录里查不到能回答这个问题的内容。"
        );
        assert_eq!(session["answer"]["sentences"], json!([]));
    }
}

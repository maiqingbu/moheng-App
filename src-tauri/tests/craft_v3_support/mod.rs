//! Shared fixtures of the CRAFT v3 end-to-end tests: a story bible, r2
//! plans that pass the checks, and a project taken through story-bible
//! confirmation.
#![allow(dead_code)]

use ai_novel_production_system_core::commands::{
    execute_command_impl, query_craft_setup_impl, query_story_directions_impl, KernelState,
};
use serde_json::{json, Value};

pub const NOW: &str = "2026-10-24T09:00:00Z";

pub fn bible() -> Value {
    json!({
        "angle":"侧重市井人情与代价",
        "normalWorld":"林满在江边码头记账，每天清点货单、和船老大讨价还价，傍晚去姑妈的面馆帮工。",
        "mechanism":{"experience":"触碰旧物时会先听见物主最后一次说话的声音。","rules":[
            {"rule":"每听一次旧物就忘掉自己一天的记忆","numbers":"一次一天，每日最多三次","howReadersSeeIt":"第二天对着账本想不起自己记过的一笔"},
            {"rule":"旧物离开原主超过一年就听不见","numbers":"一年为限","howReadersSeeIt":"当铺里的旧表一声不响"}]},
        "protagonist":{"name":"林满","personality":"嘴硬心软，算账极细","flaw":"凡事先算代价","wantsNow":"赎回姑妈的面馆","humorStyle":"一本正经地抠字眼","innerVoiceStyle":"像给自己记账，先列数再下结论"},
        "emotionalAnchor":{"object":"姑妈补过三次的围裙","memorySummary":"十岁发烧时姑妈守了他一夜"},
        "antagonist":{"name":"彭三","position":"码头放贷的二当家","wants":"吞下面馆那块地","method":"借据和打手"},
        "cast":[
            {"name":"阿秋","role":"面馆伙计","relation":"发小","privateAgenda":"想去城里学厨","speechStyle":"话少，开口就呛人","contrastWithProtagonist":"冲动","firstAppears":1},
            {"name":"老董","role":"船老大","relation":"主顾","privateAgenda":"欠彭三的钱","speechStyle":"满嘴俗语","contrastWithProtagonist":"认命","firstAppears":1}],
        "openingArc":[
            {"chapter":1,"direction":"账目里出现一笔对不上的货","protagonistWin":"当场揪出调包的货单"},
            {"chapter":2,"direction":"彭三上门催债","protagonistWin":"用旧物的声音找到借据漏洞"},
            {"chapter":3,"direction":"林满主动设局","protagonistWin":"让彭三的人当众认下假账"}]
    })
}

pub fn person(name: &str) -> Value {
    json!({"name":name,"wants":"把这笔账弄个明白","hides":"自己心里另有打算","whyHere":"每天这个时辰都在这里"})
}

/// A plan for `chapter` that passes the r2 checks; `vague` makes it fail.
pub fn craft_plan(input: &Value, chapter: i64, vague: bool) -> Value {
    let places = [
        ["江边码头", "姑妈的面馆"],
        ["当铺后院", "渡口"],
        ["彭三的账房", "江边码头"],
    ];
    let scene = |place: &str| {
        json!({"place":place,"people":[person("林满"),person("老董")],"conflict":"两人对一笔账各执一词",
            "events":["一方拿出账本","另一方指出不对","两人争到最后各退一步"],"readerQuestion":format!("第{chapter}章这笔账到底是谁做的手脚")})
    };
    let mut plan = json!({
        "payloadSchema":"chapter-plan.r2",
        "chapterId":input["chapterId"],
        "compiledFromProjectRevision":input["compiledFromProjectRevision"],
        "compiledFromStateDigest":input["compiledFromStateDigest"],
        "contextManifestRef":input["contextManifestRef"],
        "chapterGoal":format!("第{chapter}章让林满揪出一笔假账"),
        "activeTaskRefs":[],"continuityActions":[],"requiredStateOutcomes":[],"forbiddenTransitions":[],
        "knowledgeBoundaries":[],"reservationRefs":[],
        "horizonPreview":[
            {"slotNo":1,"objective":"推进当前行动","tentative":true},
            {"slotNo":2,"objective":"观察行动后果","tentative":true},
            {"slotNo":3,"objective":"准备下一冲突","tentative":true}],
        "craft":{
            "bigTurn":format!("第{chapter}章林满发现彭三的人动过货单"),
            "smallDiscovery":format!("第{chapter}章老董的账本上写着“欠彭三”"),
            "protagonistWin":format!("第{chapter}章林满当众把假账揪了出来"),
            "device":{"name":"听旧物","status":if chapter == 1 {"NEW"} else {"ESCALATED"},"howUsed":format!("第{chapter}次摸到旧物，听见的声音比上次更清楚")},
            "warmth":if vague {"温暖".to_string()} else {format!("第{chapter}章姑妈在灶上给他留了一碗面")},
            "emotionalBeat":format!("第{chapter}章他看见姑妈围裙上又多了一块补丁"),
            "situationChange":{"before":format!("第{chapter}章开头只管记账"),"after":format!("第{chapter}章结尾被彭三盯上")},
            "scenes":[scene(places[(chapter - 1) as usize][0]),scene(places[(chapter - 1) as usize][1])],
            "newLosses":[format!("第{chapter}章失去一天的记忆")],
            "endHook":format!("第{chapter}章面馆门板上贴了一张新的借据")
        }
    });
    // QC-1.2: chapter 1 names the danger closing in on the protagonist.
    if chapter == 1 {
        plan["openingThreat"] = json!({
            "event":"彭三要收走姑妈的面馆抵债",
            "deadline":"三天后的月底",
            "onPage":"彭三的伙计把催债单拍在面馆柜台上"
        });
    }
    plan
}

/// The story-directions fixture output.
pub fn directions_output() -> Value {
    let pitches = (0..3)
        .map(|index| {
            let mut pitch = json!({"name":format!("方向 {index}")});
            for field in [
                "promise",
                "protagonistDesire",
                "mechanism",
                "worldConflict",
                "longArc",
                "openingHook",
                "volumeGoal",
                "commercialEdge",
                "originality",
                "difficulty",
                "failureRisk",
            ] {
                pitch[field] = json!(format!("{field} {index}"));
            }
            pitch
        })
        .collect::<Vec<_>>();
    json!({"pitches":pitches})
}

/// An audit that finds no change.
pub fn audit_no_change() -> Value {
    json!({
        "qualityFindings":[],"canonCandidates":[],"knowledgeCandidates":[],
        "continuityCandidates":[],"taskProgressCandidates":[],
        "revealedReservationRefs":[],"evidenceRanges":[],
        "auditCompleteness":"COMPLETE_NO_CHANGE","unresolvedHard":false
    })
}

pub fn command(
    state: &KernelState,
    project_id: &str,
    id: &str,
    kind: &str,
    payload: Value,
) -> Value {
    execute_command_impl(
        state,
        json!({
            "type":kind,"commandId":id,"idempotencyKey":format!("{id}:key"),"projectId":project_id,
            "expectedRevision":state.manager.project_revision(project_id).unwrap(),
            "requestedAt":NOW,"payload":payload
        }),
    )
    .unwrap_or_else(|error| panic!("{kind} failed: {error}"))
}

/// Creates a project and takes it through story directions, story core and
/// a confirmed story bible; returns its id.
pub fn project_with_confirmed_bible(state: &KernelState, prefix: &str) -> String {
    let created = execute_command_impl(state, json!({
        "type":"project_create_from_idea","commandId":format!("{prefix}:create"),
        "idempotencyKey":format!("{prefix}:create:key"),"requestedAt":NOW,
        "payload":{"title":"码头账房","idea":"码头账房能听见旧物的声音。","platform":"番茄小说","mode":"DIRECTOR","tags":["年代"]}
    }))
    .unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    command(
        state,
        &project_id,
        &format!("{prefix}:directions"),
        "story_directions_generate",
        json!({}),
    );
    let directions = query_story_directions_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":10}),
    )
    .unwrap();
    command(
        state,
        &project_id,
        &format!("{prefix}:select"),
        "pitch_select",
        json!({"pitchIds":[directions["pitches"][0]["id"]]}),
    );
    command(
        state,
        &project_id,
        &format!("{prefix}:setup"),
        "story_setup_save",
        json!({"storyCore":{
        "protagonist":"林满","desire":"赎回姑妈的面馆","mechanism":"听旧物要付出记忆",
        "conflict":"彭三要吞面馆","readerPromise":"每章用账目和旧物赢一局","forbiddenReveals":[]}}),
    );
    command(
        state,
        &project_id,
        &format!("{prefix}:bible"),
        "story_bible_generate",
        json!({}),
    );
    let candidate = query_craft_setup_impl(state, json!({"projectId":project_id})).unwrap()
        ["storyBible"]["candidates"][0]
        .clone();
    command(
        state,
        &project_id,
        &format!("{prefix}:bible-confirm"),
        "story_bible_confirm",
        json!({"candidateId":candidate["id"],"bible":candidate}),
    );
    project_id
}

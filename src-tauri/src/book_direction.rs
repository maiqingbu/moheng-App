//! VD-1a (VOLUME_DIRECTION_DESIGN §3.1): after signing, the model reads what
//! the three opening chapters actually did and sets the book's direction and
//! the first volume's stages. The approved BookPlan keeps them in its own
//! fields; a direction that still fails its checks after the feedback rounds
//! leaves the template in place with a reminder.

use crate::craft_plan::{text, PLAN_CRAFT_ERROR};
use crate::production::ProductionModelGateway;
use crate::repositories::runtime::{DurableModelRunner, ModelRunSpec};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const PLAN_BOOK_DIRECTION: &str = "PLAN_BOOK_DIRECTION";

/// How many stages the first volume is split into.
const STAGE_COUNT: std::ops::RangeInclusive<usize> = 3..=6;

/// The direction step's input: the confirmed story, what the opening did,
/// the signing conclusions and the first volume's size.
pub fn input(
    bible: &Value,
    story_core: &Value,
    opening: &[(i64, Value)],
    signing: &Value,
    first_volume: (i64, i64),
    opening_chapters: i64,
) -> Value {
    let (start, end) = first_volume;
    json!({
        "storyBible":bible,
        "storyCore":story_core,
        "opening":opening.iter().map(|(chapter, craft)| json!({
            "chapter":chapter,
            "situationChange":craft["situationChange"],
            "bigTurn":craft["bigTurn"],
            "protagonistWin":craft["protagonistWin"],
            "endHook":craft["endHook"]
        })).collect::<Vec<_>>(),
        "signing":signing,
        "firstVolume":{
            "chapters":end - start + 1,
            "chaptersAfterOpening":end - opening_chapters
        }
    })
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn names(values: &Value) -> impl Iterator<Item = &str> {
    values.as_array().into_iter().flatten().map(text)
}

/// Whether `text` names a chapter by number, as in 第12章 or 第十章.
fn names_a_chapter(text: &str) -> bool {
    text.split('第').skip(1).any(|rest| {
        let number = rest
            .chars()
            .take_while(|c| {
                c.is_ascii_digit() || c.is_whitespace() || "零一二三四五六七八九十百".contains(*c)
            })
            .collect::<String>();
        !number.trim().is_empty() && rest[number.len()..].starts_with('章')
    })
}

fn has_chapter_table(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("chapters") || object.values().any(has_chapter_table)
        }
        Value::Array(values) => values.iter().any(has_chapter_table),
        Value::String(text) => names_a_chapter(text),
        _ => false,
    }
}

/// Everything wrong with `direction` for `input`, as feedback for the model.
pub fn problems(direction: &Value, input: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    if has_chapter_table(direction) {
        problems.push("不要写章表，也不要写“第 N 章”：阶段按情节划分，不按章号".to_string());
    }
    if text(&direction["protagonistMacroDirection"]).is_empty() {
        problems.push("protagonistMacroDirection 要写主角整本书从哪里走到哪里".to_string());
    }
    if !items(direction, "majorMysteryGoals")
        .iter()
        .any(|mystery| mystery["answerInVolume"] == 1)
    {
        problems
            .push("majorMysteryGoals 至少要有一层在第一卷揭开（answerInVolume 为 1）".to_string());
    }
    if items(&input["storyCore"], "forbiddenReveals")
        .iter()
        .any(|item| !text(item).is_empty())
        && items(direction, "futureReservations").is_empty()
    {
        problems.push("storyCore.forbiddenReveals 的内容要放进 futureReservations，写明最早第几卷才能揭开（notBeforeVolumeNo）".to_string());
    }
    let Some(first) = items(direction, "volumeIntentSeeds")
        .iter()
        .find(|seed| seed["volumeNo"] == 1)
    else {
        problems.push("volumeIntentSeeds 缺第一卷（volumeNo 为 1）".to_string());
        return problems;
    };
    if text(&first["climax"]).is_empty() {
        problems.push("第一卷要写 climax".to_string());
    }
    let stages = items(first, "stages");
    if !STAGE_COUNT.contains(&stages.len()) {
        problems.push(format!(
            "第一卷要分 {} 到 {} 个阶段，现在是 {} 个",
            STAGE_COUNT.start(),
            STAGE_COUNT.end(),
            stages.len()
        ));
    }
    let bible = &input["storyBible"];
    let known = [&bible["protagonist"]["name"], &bible["antagonist"]["name"]]
        .into_iter()
        .map(text)
        .chain(
            items(bible, "cast")
                .iter()
                .map(|member| text(&member["name"])),
        )
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    for (index, stage) in stages.iter().enumerate() {
        let number = index + 1;
        for field in ["goal", "pressure", "turn"] {
            if text(&stage[field]).is_empty() {
                problems.push(format!("第 {number} 个阶段缺 {field}"));
            }
        }
        let strangers = names(&stage["cast"])
            .filter(|name| !known.is_empty() && !known.contains(name))
            .collect::<Vec<_>>();
        if !strangers.is_empty() {
            problems.push(format!(
                "第 {number} 个阶段的 cast 里有故事圣经之外的人：{}，新人物写进 newCast",
                strangers.join("、")
            ));
        }
    }
    let estimated = stages
        .iter()
        .map(|stage| stage["estimatedChapters"].as_i64().unwrap_or(0))
        .sum::<i64>();
    let room = input["firstVolume"]["chaptersAfterOpening"]
        .as_i64()
        .unwrap_or(0);
    let least = (room + 1) / 2;
    if room > 0 && !(least..=room).contains(&estimated) {
        problems.push(format!(
            "各阶段 estimatedChapters 加起来是 {estimated} 章，第一卷开篇之后还有 {room} 章，应在 {least} 到 {room} 之间"
        ));
    }
    let antagonist = text(&bible["antagonist"]["name"]);
    if !antagonist.is_empty() && !stages.is_empty() {
        let acting = stages
            .iter()
            .filter(|stage| text(&stage["pressure"]).contains(antagonist))
            .count();
        let needed = stages.len().div_ceil(2);
        if acting < needed {
            problems.push(format!(
                "反派{antagonist}只在 {acting} 个阶段的 pressure 里出手，至少要 {needed} 个"
            ));
        }
    }
    problems
}

/// Writes `direction` into the approved plan's goal fields; `prerequisites`
/// bind its reservations to the opening evidence. The system names every
/// item.
pub fn apply(plan: &mut Map<String, Value>, direction: &Value, prerequisites: &[String]) {
    let volume = |item: &Value, key: &str| item[key].as_i64().unwrap_or(1);
    plan.insert(
        "protagonistMacroDirection".into(),
        json!(text(&direction["protagonistMacroDirection"])),
    );
    let numbered = |key: &str, build: &dyn Fn(usize, &Value) -> Value| {
        Value::Array(
            items(direction, key)
                .iter()
                .enumerate()
                .map(|(index, item)| build(index + 1, item))
                .collect(),
        )
    };
    plan.insert(
        "mustAchieve".into(),
        numbered("mustAchieve", &|n, goal| {
            json!({"id":format!("goal:book:{n}"),"volumeNo":volume(goal, "volumeNo"),"description":text(&goal["description"])})
        }),
    );
    plan.insert(
        "relationshipMacroDirections".into(),
        numbered("relationshipMacroDirections", &|n, relation| {
            json!({"id":format!("relation:book:{n}"),"volumeNo":volume(relation, "volumeNo"),
                "description":format!("{}：从{}走向{}",
                    names(&relation["between"]).collect::<Vec<_>>().join("与"),
                    text(&relation["from"]), text(&relation["toward"]))})
        }),
    );
    plan.insert(
        "majorMysteryGoals".into(),
        numbered("majorMysteryGoals", &|n, mystery| {
            json!({"id":format!("mystery:book:{n}"),"volumeNo":volume(mystery, "answerInVolume"),
                "description":format!("第 {} 层：{}", volume(mystery, "layer"), text(&mystery["description"]))})
        }),
    );
    plan.insert(
        "futureReservations".into(),
        numbered("futureReservations", &|n, reservation| {
            let not_before = reservation["notBeforeVolumeNo"].as_i64().unwrap_or(2);
            json!({"reservationId":format!("reservation:book:{n}"),"description":text(&reservation["description"]),
                "notBeforeVolumeNo":not_before,"reservedForVolumeNo":not_before,"prerequisites":prerequisites,
                "mayForeshadow":true,"mayReveal":false,"authorityLevel":"B"})
        }),
    );
    plan.insert(
        "volumeIntentSeeds".into(),
        items(direction, "volumeIntentSeeds").iter().map(|seed| {
            let volume_no = volume(seed, "volumeNo");
            let mut built = json!({"seedId":format!("volume-{volume_no}"),"description":text(&seed["description"])});
            let stages = items(seed, "stages");
            if !stages.is_empty() {
                built["stages"] = stages.iter().enumerate().map(|(index, stage)| {
                    let mut built = json!({"id":format!("stage:{volume_no}:{}", index + 1),
                        "goal":text(&stage["goal"]),"pressure":text(&stage["pressure"]),
                        "cast":names(&stage["cast"]).collect::<Vec<_>>(),"turn":text(&stage["turn"]),
                        "estimatedChapters":stage["estimatedChapters"].as_i64().unwrap_or(1).max(1)});
                    let new_cast = names(&stage["newCast"]).collect::<Vec<_>>();
                    if !new_cast.is_empty() {
                        built["newCast"] = json!(new_cast);
                    }
                    built
                }).collect();
            }
            if !text(&seed["climax"]).is_empty() {
                built["climax"] = json!(text(&seed["climax"]));
            }
            built
        }).collect::<Vec<_>>().into(),
    );
}

/// What planning the direction gave: the direction and the run that wrote
/// it, or the reminder that the template stays. Gateways that do not plan
/// directions give neither.
#[derive(Default)]
pub struct Planned {
    pub direction: Option<Value>,
    pub model_run_id: Option<String>,
    pub reminder: Option<Value>,
}

/// What the signing evaluation concluded, for the direction step.
fn signing_conclusions(receipt: &Value) -> Value {
    let concerns = items(receipt, "itemResults")
        .iter()
        .filter(|item| item["result"] != "PASS")
        .map(|item| text(&item["reason"]))
        .collect::<Vec<_>>();
    let passed = items(receipt, "platformMatrix")
        .iter()
        .filter(|platform| platform["status"] == "PASS")
        .map(|platform| text(&platform["displayName"]))
        .collect::<Vec<_>>();
    json!({"verdict":text(&receipt["verdict"]),"passedPlatforms":passed,"concerns":concerns})
}

/// Asks the model for the book's direction from the opening and checks it,
/// sending its problems back up to the plan feedback rounds; what still
/// fails, or any failed run, keeps the template with a reminder.
pub fn plan(
    connection: &Connection,
    gateway: &dyn ProductionModelGateway,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    now: &str,
) -> Result<Planned, String> {
    if !gateway.plans_book_direction() {
        return Ok(Planned::default());
    }
    let head = |artifact_type: &str| {
        crate::repositories::project_entry::canonical_head(connection, project_id, artifact_type)
            .map(|row| row.map(|row| row.payload).unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    };
    let bible = crate::craft_plan::confirmed_bible(connection, project_id)?.unwrap_or(Value::Null);
    let opening_chapters = crate::repositories::project_entry::OPENING_CHAPTER_COUNT;
    let opening = crate::repositories::craft::plan_crafts_before(
        connection,
        project_id,
        opening_chapters + 1,
    )
    .map_err(|error| error.to_string())?;
    let first_volume =
        crate::repositories::volume_lifecycle::VolumeLifecycleRepository::volume_chapter_range(
            connection, project_id, 1,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
    let input = input(
        &bible,
        &head("StoryConstitution")?["storyCore"],
        &opening,
        &signing_conclusions(&head("SimulatedSigningReceipt")?),
        first_volume,
        opening_chapters,
    );
    let profile = gateway.resolve_step_profile(PLAN_BOOK_DIRECTION)?;
    let spec = ModelRunSpec {
        model_run_id: StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("{command_id}:book-direction"),
        )?,
        job_id: None,
        step_id: None,
        provider: profile.provider.clone(),
        model: profile.model.clone(),
        idempotency_key: format!("{idempotency_key}:book-direction"),
        capability: PLAN_BOOK_DIRECTION.into(),
        prompt_ref: None,
        context_manifest_ref: None,
        budget_id: Some(DurableModelRunner::project_budget_id(project_id)),
        predicted_cost_cents: 0,
        cost_snapshot: None,
        attempt_no: 1,
        repair_round: 0,
    };
    let accept = |direction: Value, lenient: bool| {
        let found = problems(&direction, &input);
        match (found.is_empty(), lenient) {
            (true, _) => Ok((direction, found)),
            (false, true) => Ok((Value::Null, found)),
            (false, false) => Err(format!("{PLAN_CRAFT_ERROR}:{}", found.join("；"))),
        }
    };
    let reason = match crate::production::plan_chapter_with_feedback(
        connection,
        gateway,
        &profile,
        &spec,
        input.clone(),
        now,
        &accept,
    ) {
        Ok((direction, model_run_id, _)) if !direction.is_null() => {
            return Ok(Planned {
                direction: Some(direction),
                model_run_id: Some(model_run_id),
                reminder: None,
            })
        }
        Ok((_, _, problems)) => problems.join("；"),
        Err(error) => error,
    };
    Ok(Planned {
        reminder: Some(template_reminder(command_id, project_id, &reason)?),
        ..Planned::default()
    })
}

/// The reminder that the book plan kept its template goals.
fn template_reminder(command_id: &str, project_id: &str, reason: &str) -> Result<Value, String> {
    Ok(json!({
        "decisionId":StableIdentityFactory::from_command(
            StableIdKind::Decision,
            &format!("vd1-direction-reminder:{command_id}"),
        )?,
        "decisionType":"AUDIT_REMINDER",
        "outcome":"OPEN",
        "reason":format!("全书方向没有生成，沿用了模板：{reason}"),
        "sourceRef":format!("plan:bookplan:{project_id}"),
        "payloadJson":{
            "category":"规划提醒",
            "title":"全书方向没有生成，第一卷没有阶段目标",
            "heldBack":[{"field":"plan","item":reason}],
            "allowedActions":["ACKNOWLEDGE"]
        }
    }))
}

/// VD-1c: where chapter `chapter_no` stands in volume `volume_id`'s stages,
/// for the planner: the first stage whose task is not completed, how many
/// chapters it has had since the stage before it ended (the first stage
/// counts from `before_stages`), and the stages after it. `completed` maps
/// each satisfied task to the chapter that completed it. None when the
/// volume has no stages.
pub fn chapter_direction(
    book_plan: &Value,
    route: &Value,
    (volume_id, volume_no): (&str, i64),
    before_stages: i64,
    completed: &BTreeMap<String, i64>,
    chapter_no: i64,
) -> Option<Value> {
    let seed = crate::planning::volume_seeds(book_plan, volume_no).next()?;
    let task_for = |stage: &Value| {
        let task_id = crate::planning::stage_task_id(volume_id, stage["id"].as_str()?);
        items(route, "tasks")
            .iter()
            .any(|task| task["taskId"] == task_id.as_str())
            .then_some(task_id)
    };
    let stages = items(seed, "stages")
        .iter()
        .filter_map(|stage| Some((stage, task_for(stage)?)))
        .collect::<Vec<_>>();
    if stages.is_empty() {
        return None;
    }
    let current = stages
        .iter()
        .position(|(_, task)| !completed.contains_key(task))
        .unwrap_or(stages.len());
    let started = current
        .checked_sub(1)
        .and_then(|previous| completed.get(&stages[previous].1).copied())
        .unwrap_or(before_stages);
    let current_stage = stages.get(current).map(|(stage, task)| {
        let mut stage = (*stage).clone();
        stage["taskId"] = json!(task);
        stage["chaptersSoFar"] = json!((chapter_no - 1 - started).max(0));
        stage
    });
    Some(json!({
        "volumeFunction":text(&seed["description"]),
        "currentStage":current_stage,
        "laterStages":stages.iter().skip(current + 1).map(|(stage, _)| text(&stage["goal"])).collect::<Vec<_>>(),
        "climax":text(&seed["climax"])
    }))
}

/// The direction for planning `chapter_id` (chapter `chapter_no`), from the
/// approved BookPlan and its volume's route; None before the route exists or
/// when the volume has no stages.
pub fn for_chapter(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
    chapter_no: i64,
) -> Result<Option<Value>, String> {
    use crate::repositories::narrative::NarrativeRepository;
    let Some(volume_id) =
        NarrativeRepository::chapter_volume_id(connection, project_id, chapter_id)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(volume) =
        crate::repositories::volume_lifecycle::VolumeLifecycleRepository::volume_metadata(
            connection, project_id, &volume_id,
        )
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let payload = |head: Result<Option<_>, _>| {
        head.and_then(|reference| {
            reference
                .map(|reference| {
                    NarrativeRepository::artifact_payload_by_ref(connection, project_id, &reference)
                })
                .transpose()
                .map(Option::flatten)
        })
        .map_err(|error| error.to_string())
    };
    let book_plan = payload(NarrativeRepository::canonical_project_head_ref(
        connection, project_id, "BookPlan",
    ))?;
    let route = payload(NarrativeRepository::canonical_head_ref(
        connection,
        project_id,
        "VolumePlan",
        &volume_id,
    ))?;
    let (Some(book_plan), Some(route)) = (book_plan, route) else {
        return Ok(None);
    };
    let completed =
        NarrativeRepository::satisfied_task_chapters(connection, project_id, chapter_no)
            .map_err(|error| error.to_string())?;
    // The first volume's stages begin after the opening chapters.
    let before_stages = match volume.volume_no {
        1 => crate::repositories::project_entry::OPENING_CHAPTER_COUNT,
        _ => volume.chapter_start - 1,
    };
    Ok(chapter_direction(
        &book_plan,
        &route,
        (&volume_id, volume.volume_no),
        before_stages,
        &completed,
        chapter_no,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bible() -> Value {
        json!({
            "protagonist":{"name":"陆照"},
            "antagonist":{"name":"裴砚舟"},
            "cast":[{"name":"柳漪","firstAppears":1},{"name":"陈伯","firstAppears":1}]
        })
    }

    fn direction_input() -> Value {
        let opening = [(1, json!({"endHook":"账册上悬着陆照的名字"}))];
        input(
            &bible(),
            &json!({"readerPromise":"以命换力","forbiddenReveals":["租约幕后的操盘者"]}),
            &opening,
            &json!({"verdict":"通过"}),
            (1, 36),
            3,
        )
    }

    fn stage(goal: &str, pressure: &str, cast: &[&str], chapters: i64) -> Value {
        json!({"goal":goal,"pressure":pressure,"cast":cast,"turn":"段末转折","estimatedChapters":chapters})
    }

    fn direction() -> Value {
        json!({
            "protagonistMacroDirection":"陆照从任人差遣的杂役走到替宗门签下新约的人",
            "mustAchieve":[{"volumeNo":1,"description":"陆照拿到租约的原件"}],
            "relationshipMacroDirections":[{"between":["陆照","柳漪"],"from":"互相提防","toward":"并肩查账","volumeNo":1}],
            "majorMysteryGoals":[{"layer":1,"answerInVolume":1,"description":"租约里多出的那一条是谁加的"}],
            "futureReservations":[{"description":"租约幕后的操盘者","notBeforeVolumeNo":3}],
            "volumeIntentSeeds":[
                {"volumeNo":1,"description":"守住丹房，揪出改约的人","stages":[
                    stage("陆照查清丹房的账","裴砚舟扣下陆照的月例",&["陆照","陈伯"],8),
                    stage("陆照找到改约的证据","裴砚舟设局栽赃陆照",&["柳漪"],10),
                    stage("陆照在会审上翻案","三大宗派人压阵",&["陆照"],10)
                ],"climax":"会审上陆照当众拿出原件"},
                {"volumeNo":2,"description":"走出青崖宗"}
            ]
        })
    }

    /// The approved plan and route VD-1a and VD-1b make from `direction()`.
    fn plan_and_route() -> (Value, Value) {
        let mut plan = Map::new();
        apply(&mut plan, &direction(), &[]);
        let stage_task = |n: usize| json!({"taskId":format!("task:required:goal:vol:1:stage:1:{n}"),"class":"REQUIRED"});
        let route = json!({"tasks":[
            stage_task(1), stage_task(2), stage_task(3),
            {"taskId":"task:required:goal:book:1","class":"REQUIRED"}
        ]});
        (Value::Object(plan), route)
    }

    #[test]
    fn chapter_four_starts_the_first_stage() {
        let (plan, route) = plan_and_route();
        let direction =
            chapter_direction(&plan, &route, ("vol:1", 1), 3, &BTreeMap::new(), 4).unwrap();
        assert_eq!(direction["volumeFunction"], "守住丹房，揪出改约的人");
        let stage = &direction["currentStage"];
        assert_eq!(stage["taskId"], "task:required:goal:vol:1:stage:1:1");
        assert_eq!(stage["goal"], "陆照查清丹房的账");
        assert_eq!(stage["pressure"], "裴砚舟扣下陆照的月例");
        assert_eq!(stage["cast"], json!(["陆照", "陈伯"]));
        assert_eq!(stage["turn"], "段末转折");
        assert_eq!(stage["estimatedChapters"], 8);
        assert_eq!(stage["chaptersSoFar"], 0);
        assert_eq!(
            direction["laterStages"],
            json!(["陆照找到改约的证据", "陆照在会审上翻案"])
        );
        assert_eq!(direction["climax"], "会审上陆照当众拿出原件");
    }

    #[test]
    fn a_completed_stage_moves_the_chapter_on_and_counts_from_its_end() {
        let (plan, route) = plan_and_route();
        let completed = BTreeMap::from([("task:required:goal:vol:1:stage:1:1".to_string(), 9)]);
        let direction = chapter_direction(&plan, &route, ("vol:1", 1), 3, &completed, 12).unwrap();
        assert_eq!(direction["currentStage"]["goal"], "陆照找到改约的证据");
        assert_eq!(direction["currentStage"]["chaptersSoFar"], 2);
        assert_eq!(direction["laterStages"], json!(["陆照在会审上翻案"]));
    }

    #[test]
    fn after_the_last_stage_only_the_climax_is_left() {
        let (plan, route) = plan_and_route();
        let completed = (1..=3)
            .map(|n| (format!("task:required:goal:vol:1:stage:1:{n}"), 10 * n))
            .collect();
        let direction = chapter_direction(&plan, &route, ("vol:1", 1), 3, &completed, 33).unwrap();
        assert_eq!(direction["currentStage"], Value::Null);
        assert_eq!(direction["laterStages"], json!([]));
        assert_eq!(direction["climax"], "会审上陆照当众拿出原件");
    }

    #[test]
    fn a_volume_without_stages_has_no_direction() {
        let (plan, route) = plan_and_route();
        assert_eq!(
            chapter_direction(&plan, &route, ("vol:2", 2), 36, &BTreeMap::new(), 40),
            None
        );
    }

    #[test]
    fn a_direction_with_three_stages_that_fit_the_volume_has_no_problems() {
        assert_eq!(
            problems(&direction(), &direction_input()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_input_holds_what_the_opening_did_and_the_chapters_left_in_the_volume() {
        let input = direction_input();
        assert_eq!(input["storyBible"]["antagonist"]["name"], "裴砚舟");
        assert_eq!(input["opening"][0]["chapter"], 1);
        assert_eq!(input["opening"][0]["endHook"], "账册上悬着陆照的名字");
        assert_eq!(input["firstVolume"]["chaptersAfterOpening"], 33);
    }

    #[test]
    fn a_chapter_table_or_a_chapter_number_is_a_problem() {
        let mut numbered = direction();
        numbered["volumeIntentSeeds"][0]["stages"][1]["turn"] = json!("第12章裴砚舟倒台");
        let mut table = direction();
        table["chapters"] = json!([{"no":4}]);
        for direction in [numbered, table] {
            assert!(problems(&direction, &direction_input())
                .iter()
                .any(|problem| problem.contains("不要写章表")));
        }
        let mut chinese = direction();
        chinese["volumeIntentSeeds"][0]["climax"] = json!("第十章会审");
        assert!(problems(&chinese, &direction_input())
            .iter()
            .any(|problem| problem.contains("不要写章表")));
    }

    #[test]
    fn two_stages_or_seven_are_a_problem() {
        for count in [2, 7] {
            let mut direction = direction();
            direction["volumeIntentSeeds"][0]["stages"] = json!((0..count)
                .map(|_| stage("目标", "裴砚舟出手", &[], 3))
                .collect::<Vec<_>>());
            assert!(problems(&direction, &direction_input())
                .contains(&format!("第一卷要分 3 到 6 个阶段，现在是 {count} 个")));
        }
    }

    #[test]
    fn a_stage_without_a_turn_is_a_problem() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][2]["turn"] = json!(" ");
        assert!(
            problems(&direction, &direction_input()).contains(&"第 3 个阶段缺 turn".to_string())
        );
    }

    #[test]
    fn stage_estimates_beyond_the_volume_are_a_problem() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][0]["estimatedChapters"] = json!(20);
        assert!(problems(&direction, &direction_input()).contains(
            &"各阶段 estimatedChapters 加起来是 40 章，第一卷开篇之后还有 33 章，应在 17 到 33 之间"
                .to_string()
        ));
    }

    #[test]
    fn a_stage_cast_member_outside_the_bible_must_be_marked_new() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][1]["cast"] = json!(["柳漪", "沈掌柜"]);
        assert!(problems(&direction, &direction_input()).contains(
            &"第 2 个阶段的 cast 里有故事圣经之外的人：沈掌柜，新人物写进 newCast".to_string()
        ));
        direction["volumeIntentSeeds"][0]["stages"][1]["cast"] = json!(["柳漪"]);
        direction["volumeIntentSeeds"][0]["stages"][1]["newCast"] = json!(["沈掌柜"]);
        assert_eq!(
            problems(&direction, &direction_input()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_antagonist_acting_in_fewer_than_half_the_stages_is_a_problem() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][1]["pressure"] = json!("宗门断了丹房的炭");
        assert!(problems(&direction, &direction_input())
            .contains(&"反派裴砚舟只在 1 个阶段的 pressure 里出手，至少要 2 个".to_string()));
    }

    #[test]
    fn forbidden_reveals_need_a_reservation_and_the_first_volume_a_mystery_layer() {
        let mut direction = direction();
        direction["futureReservations"] = json!([]);
        direction["majorMysteryGoals"][0]["answerInVolume"] = json!(2);
        let found = problems(&direction, &direction_input());
        assert!(found
            .iter()
            .any(|problem| problem.contains("forbiddenReveals")));
        assert!(found
            .iter()
            .any(|problem| problem.contains("answerInVolume 为 1")));
    }

    #[test]
    fn a_missing_first_volume_or_climax_is_a_problem() {
        let mut no_climax = direction();
        no_climax["volumeIntentSeeds"][0]
            .as_object_mut()
            .unwrap()
            .remove("climax");
        assert!(problems(&no_climax, &direction_input()).contains(&"第一卷要写 climax".to_string()));
        let mut no_first = direction();
        no_first["volumeIntentSeeds"][0]["volumeNo"] = json!(3);
        assert!(problems(&no_first, &direction_input())
            .contains(&"volumeIntentSeeds 缺第一卷（volumeNo 为 1）".to_string()));
    }

    #[test]
    fn the_direction_fills_the_plan_with_system_named_items() {
        let mut plan = Map::new();
        plan.insert(
            "protagonistMacroDirection".into(),
            json!("青崖宗外门丹房杂役陆照"),
        );
        apply(&mut plan, &direction(), &["canon:1".to_string()]);
        assert_eq!(
            plan["protagonistMacroDirection"],
            "陆照从任人差遣的杂役走到替宗门签下新约的人"
        );
        assert_eq!(
            plan["mustAchieve"],
            json!([{"id":"goal:book:1","volumeNo":1,"description":"陆照拿到租约的原件"}])
        );
        assert_eq!(
            plan["relationshipMacroDirections"][0],
            json!({"id":"relation:book:1","volumeNo":1,"description":"陆照与柳漪：从互相提防走向并肩查账"})
        );
        assert_eq!(
            plan["majorMysteryGoals"][0],
            json!({"id":"mystery:book:1","volumeNo":1,"description":"第 1 层：租约里多出的那一条是谁加的"})
        );
        assert_eq!(
            plan["futureReservations"][0],
            json!({"reservationId":"reservation:book:1","description":"租约幕后的操盘者",
                "notBeforeVolumeNo":3,"reservedForVolumeNo":3,"prerequisites":["canon:1"],
                "mayForeshadow":true,"mayReveal":false,"authorityLevel":"B"})
        );
        let seeds = plan["volumeIntentSeeds"].as_array().unwrap();
        assert_eq!(seeds[0]["seedId"], "volume-1");
        assert_eq!(seeds[0]["climax"], "会审上陆照当众拿出原件");
        assert_eq!(seeds[0]["stages"][1]["id"], "stage:1:2");
        assert_eq!(seeds[0]["stages"][1]["pressure"], "裴砚舟设局栽赃陆照");
        assert_eq!(
            seeds[1],
            json!({"seedId":"volume-2","description":"走出青崖宗"})
        );
        let approved = json!({
            "payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","storyPromise":"以命换力",
            "endingBoundary":"结局","mustNotBreak":[],"openingHypotheses":[],
            "globalLengthBudget":{"preferredWords":1_200_000}
        });
        let mut approved = approved.as_object().unwrap().clone();
        approved.extend(plan);
        crate::generated::validators::validate_book_plan_r1(&Value::Object(approved)).unwrap();
    }
}

//! WM-1 phase 1 (owner decision 2026-11-01, WORK_MEMORY_DESIGN §4): what a
//! chapter is written with is recalled from the work's memory by what the
//! chapter plans, not only by how recent it is. Offline and deterministic.

use ai_novel_production_system_core::work_memory::{recall, Memory, RecallQuery};
use std::time::Instant;

fn memory(chapter: i64, subject: &str, kind: &str, detail: &str) -> Memory {
    Memory {
        chapter,
        subject: subject.into(),
        kind: kind.into(),
        detail: detail.into(),
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

/// Six details a chapter about nothing in particular, for `chapters`
/// chapters, newest last.
fn filler(chapters: i64) -> Vec<Memory> {
    (1..=chapters)
        .flat_map(|chapter| {
            (0..6).map(move |index| {
                memory(
                    chapter,
                    "路人",
                    "OBJECT",
                    &format!("第{chapter}章的第{index}件杂物"),
                )
            })
        })
        .collect()
}

fn details(recalled: &[serde_json::Value]) -> Vec<&str> {
    recalled
        .iter()
        .map(|item| item["detail"].as_str().unwrap())
        .collect()
}

#[test]
fn wm1_the_people_on_stage_come_first_and_newest_first() {
    let memories = vec![
        memory(1, "林薇", "HABIT", "喝黑咖啡不加糖"),
        memory(2, "钟楼", "PLACE", "钟楼顶层的窗玻璃碎了一角"),
        memory(3, "沈砚", "HABIT", "进门先看表"),
        memory(4, "沈砚", "APPEARANCE", "左手缠着纱布"),
    ];
    let on_stage = names(&["沈砚"]);
    let people = names(&["沈砚", "林薇"]);
    let query = RecallQuery {
        on_stage: &on_stage,
        people: &people,
        text: "沈砚在档案室翻账本",
    };
    let recalled = recall(&memories, &query, 40);
    assert_eq!(
        details(&recalled),
        vec!["左手缠着纱布", "进门先看表", "钟楼顶层的窗玻璃碎了一角"]
    );
    assert_eq!(recalled[0]["chapter"], 4);
    assert_eq!(recalled[0]["subject"], "沈砚");
    assert_eq!(recalled[0]["kind"], "APPEARANCE");
}

#[test]
fn wm1_a_later_detail_of_the_same_thing_comes_before_the_earlier_one() {
    // 时间敏感: the wound of chapter 3 healed in chapter 9.
    let memories = vec![
        memory(3, "沈砚", "APPEARANCE", "左手缠着纱布"),
        memory(9, "沈砚", "APPEARANCE", "左手的纱布拆了，留下一道疤"),
    ];
    let on_stage = names(&["沈砚"]);
    let query = RecallQuery {
        on_stage: &on_stage,
        people: &on_stage,
        text: "",
    };
    let recalled = recall(&memories, &query, 40);
    assert_eq!(recalled[0]["chapter"], 9);
    assert_eq!(recalled[1]["chapter"], 3, "the earlier one stays traceable");
}

#[test]
fn wm1_a_setup_long_ago_is_recalled_when_the_plan_brings_it_back() {
    // 久远设定: chapter 5's object, chapter 300's plan names it, and 1,800
    // newer details stand in between.
    let mut memories = vec![memory(5, "铜钥匙", "OBJECT", "钥匙柄上刻着一只缺耳的猫")];
    memories.extend(filler(299));
    let on_stage = names(&["沈砚"]);
    let query = RecallQuery {
        on_stage: &on_stage,
        people: &on_stage,
        text: "沈砚在旧抽屉里找到那把铜钥匙",
    };
    let recalled = recall(&memories, &query, 40);
    assert_eq!(recalled.len(), 40);
    assert_eq!(recalled[0]["detail"], "钥匙柄上刻着一只缺耳的猫");
}

#[test]
fn wm1_an_absent_person_s_traits_are_never_recalled_but_their_things_may_be() {
    // 张冠李戴 (the worst error): 林薇 is not in this chapter. Her habit
    // never comes in, even when the plan's words touch it; her cup does
    // when the plan uses it, still marked as hers.
    let memories = vec![
        memory(2, "林薇", "HABIT", "每天喝黑咖啡"),
        memory(2, "林薇", "APPEARANCE", "短发别着铜发夹"),
        memory(3, "林薇", "OBJECT", "搪瓷咖啡杯缺了一个耳朵"),
    ];
    let on_stage = names(&["沈砚"]);
    let people = names(&["沈砚", "林薇"]);
    let query = RecallQuery {
        on_stage: &on_stage,
        people: &people,
        text: "沈砚端起那只搪瓷咖啡杯，喝了一口黑咖啡",
    };
    let recalled = recall(&memories, &query, 40);
    assert_eq!(details(&recalled), vec!["搪瓷咖啡杯缺了一个耳朵"]);
    assert_eq!(recalled[0]["subject"], "林薇");
}

#[test]
fn wm1_a_person_the_plan_mentions_counts_as_in_the_chapter() {
    let memories = vec![memory(2, "林薇", "HABIT", "每天喝黑咖啡")];
    let on_stage = names(&["沈砚"]);
    let people = names(&["沈砚", "林薇"]);
    let query = RecallQuery {
        on_stage: &on_stage,
        people: &people,
        text: "沈砚想起林薇说过的话",
    };
    assert_eq!(
        details(&recall(&memories, &query, 40)),
        vec!["每天喝黑咖啡"]
    );
}

#[test]
fn wm1_a_detail_stated_again_is_kept_once_at_its_newest() {
    let memories = vec![
        memory(1, "沈砚", "HABIT", "进门先看表"),
        memory(6, "沈砚", "HABIT", "进门先看表"),
    ];
    let on_stage = names(&["沈砚"]);
    let query = RecallQuery {
        on_stage: &on_stage,
        people: &on_stage,
        text: "",
    };
    let recalled = recall(&memories, &query, 40);
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0]["chapter"], 6);
}

#[test]
fn wm1_three_thousand_chapters_of_memory_are_recalled_quickly() {
    // 规模: about six details a chapter for 3,000 chapters.
    let mut memories = filler(3000);
    memories.push(memory(12, "铜钥匙", "OBJECT", "钥匙柄上刻着一只缺耳的猫"));
    let on_stage = names(&["沈砚"]);
    let query = RecallQuery {
        on_stage: &on_stage,
        people: &on_stage,
        text: "沈砚在旧抽屉里找到那把铜钥匙",
    };
    let started = Instant::now();
    let recalled = recall(&memories, &query, 40);
    let took = started.elapsed();
    assert_eq!(recalled[0]["detail"], "钥匙柄上刻着一只缺耳的猫");
    assert!(took.as_millis() < 1000, "recall took {took:?}");
}

//! CRAFT v3 step 4a: continuity details from the audit (habits, preferences,
//! appearance, objects, places, relations) and the established facts the
//! next chapter is planned and written with (CRAFT_V3_DESIGN §4.3).

use ai_novel_production_system_core::craft_plan::{
    continuity_details, established_facts, MAX_ESTABLISHED_FACTS,
};
use serde_json::{json, Value};

const BODY: &str = "陈姐把豆浆递过来：“还是不加糖？”\n林满点头，把三块钱压在碗底。";

fn detail(subject: &str, kind: &str, detail: &str, quote: &str) -> Value {
    json!({"subject":subject,"kind":kind,"detail":detail,"quote":quote})
}

#[test]
fn only_complete_known_and_quoted_details_are_kept_without_their_quotes() {
    let audit = json!({"continuityDetails":[
        detail("林满", "PREFERENCE", "喝豆浆不加糖", "还是不加糖？"),
        detail("林满", "HABIT", "把钱压在碗底付账", "林满点头， 把三块钱压在碗底"),
        detail("林满", "MOOD", "心情不好", "林满点头"),
        detail("", "HABIT", "没有主语", "林满点头"),
        detail("陈姐", "APPEARANCE", "围着蓝围裙", "陈姐系着蓝围裙")
    ]});
    let kept = continuity_details(&audit, BODY);
    assert_eq!(
        kept,
        vec![
            json!({"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖"}),
            json!({"subject":"林满","kind":"HABIT","detail":"把钱压在碗底付账"}),
        ]
    );
    assert!(continuity_details(&json!({}), BODY).is_empty());
}

#[test]
fn established_facts_are_newest_first_deduplicated_and_focused_on_this_chapter() {
    let prior = vec![
        (
            1,
            json!([{"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖"},
                   {"subject":"陈姐","kind":"APPEARANCE","detail":"围着蓝围裙"}]),
        ),
        (
            2,
            json!([{"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖"},
                   {"subject":"老董","kind":"HABIT","detail":"说话前先咳一声"}]),
        ),
    ];
    let facts = established_facts(&prior, &["陈姐".to_string()]);
    let subjects = facts
        .iter()
        .map(|fact| fact["subject"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(subjects, vec!["陈姐", "林满", "老董"]);
    assert_eq!(
        facts[1]["chapter"], 2,
        "the newest statement of a repeated fact is kept"
    );

    let many = (1..=30)
        .map(|chapter| (chapter, json!([{"subject":"林满","kind":"OBJECT","detail":format!("第{chapter}件旧物")},
                                       {"subject":"老董","kind":"OBJECT","detail":format!("第{chapter}张货单")}])))
        .collect::<Vec<_>>();
    let bounded = established_facts(&many, &[]);
    assert_eq!(bounded.len(), MAX_ESTABLISHED_FACTS);
    assert_eq!(bounded[0]["chapter"], 30);
}

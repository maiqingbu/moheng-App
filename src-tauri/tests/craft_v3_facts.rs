//! CRAFT v3 step 4a: continuity details from the audit (habits, preferences,
//! appearance, objects, places, relations) and the established facts the
//! next chapter is planned and written with (CRAFT_V3_DESIGN §4.3).

use ai_novel_production_system_core::craft_plan::continuity_details;
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
fn a_place_detail_keeps_the_following_text_when_its_state_changes_later() {
    // Round 33, chapter 8: the audit cited the earlier condition but the
    // body goes on to show the water crossing the rim.
    let body = "辰时将近，坛沿浊黄水光已漫到最高处，鼓着，颤着，就是不溢。表面霜纹凝滞，像被什么压住。哑婆婆赤足踏上坛基，鹤羽一倾。那滴淡金露水坠下，正中坛口一道细微凹痕。陆照伸手欲接，半途停住，改了主意。他侧过脸，把左耳裂痕贴向坛壁。嗡鸣陡然拔高一阶。坛面霜纹活过来，蛛网裂开，浊黄水光向上拱起一寸，漫过沿口，淌下第一道水线。";
    let audit = json!({"continuityDetails":[detail(
        "废丹坛", "PLACE", "坛沿浊黄水光已漫到最高处，鼓着，颤着，就是不溢；表面霜纹凝滞，像被什么压住",
        "辰时将近，坛沿浊黄水光已漫到最高处，鼓着，颤着，就是不溢。表面霜纹凝滞，像被什么压住。"
    )]});
    let kept = continuity_details(&audit, body);
    assert!(kept[0]["detail"]
        .as_str()
        .unwrap()
        .contains("浊黄水光向上拱起一寸，漫过沿口"));
}

#[test]
fn an_ambiguous_quote_does_not_claim_which_occurrence_comes_first() {
    let body = "水光颤动。后来水光颤动。随后水漫出来。";
    let audit = json!({"continuityDetails":[detail("废丹坛", "PLACE", "水光颤动", "水光颤动。") ]});
    assert_eq!(continuity_details(&audit, body)[0]["detail"], "水光颤动");
}

#[test]
fn following_text_is_bounded_and_identified_as_unconfirmed() {
    let body = format!("水光颤动。{}", "水".repeat(300));
    let audit = json!({"continuityDetails":[detail("废丹坛", "PLACE", "水光颤动", "水光颤动。") ]});
    let kept = continuity_details(&audit, &body);
    let detail = kept[0]["detail"].as_str().unwrap();
    assert!(detail.contains("可能早于章末"));
    assert!(detail.contains("未确认章末状态"));
    assert!(detail.contains("可能有省略"));
    assert_eq!(detail.chars().filter(|c| *c == '水').count(), 161);
}

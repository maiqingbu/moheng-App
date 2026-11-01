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

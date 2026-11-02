//! RP-1 (real runs 13-15, 2026-11-02): chapter endings and catchphrases
//! repeated across chapters, one use in each.

use ai_novel_production_system_core::chapter_craft::{
    catchphrase_candidates, ending_repetition_findings,
};

fn texts(items: &[&str]) -> Vec<String> {
    items.iter().map(|text| text.to_string()).collect()
}

#[test]
fn rp1_a_phrase_used_once_in_three_of_four_chapters_is_a_catchphrase_candidate() {
    // Newest first, as ChapterHistory holds them.
    let history = texts(&[
        "陈伯拄着拐站定。“这火候啊，还不到。”陆照没吭声。",
        "灶间里很静。陆照把药渣倒掉，擦干了手。",
        "陈伯用拐杖拨了拨陶管：“这火候啊，我瞧着沉了三分。”",
    ]);
    let body = "陈伯把拐杖往地上一顿：“这火候啊，再早药性就跑了。”陆照点头。";
    let candidates = catchphrase_candidates(&history, body, &["陈伯", "陆照"], "");
    let found = candidates
        .iter()
        .find(|candidate| candidate["phrase"] == "这火候啊")
        .unwrap_or_else(|| panic!("{candidates:?}"));
    assert_eq!(found["thisChapter"], 1);
    assert_eq!(found["chapters"], 3);
}

#[test]
fn rp1_a_phrase_in_only_two_of_four_chapters_is_not_a_candidate() {
    let history = texts(&[
        "灶间里很静。陆照把药渣倒掉。",
        "陆照擦干了手，转身出门。",
        "陈伯用拐杖拨了拨陶管：“这火候啊，沉了三分。”",
    ]);
    let body = "陈伯把拐杖往地上一顿：“这火候啊，再早药性就跑了。”";
    let candidates = catchphrase_candidates(&history, body, &["陈伯"], "");
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate["phrase"] == "这火候啊"),
        "{candidates:?}"
    );
}

#[test]
fn rp1_an_ending_like_a_recent_ones_is_a_rewrite_finding_on_the_last_sentence() {
    let earlier = "他走出药庐，把炭条断口含进嘴里，苦味钻到舌根，泛起回甘的暖意。左手摊开，掌心第七枚残渣的灰纹转着。";
    let body = "陆照没再说话。他把炭条断口含进嘴里，苦味钻到舌根，回甘慢慢浮上来。摊开左手，掌心残渣的灰纹还在转。";
    let findings = ending_repetition_findings(&texts(&["别的章节结尾是风吹灯灭。", earlier]), body);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["code"], "ending_repetition");
    assert_eq!(findings[0]["quote"], "摊开左手，掌心残渣的灰纹还在转。");
    assert!(findings[0]["problem"]
        .as_str()
        .unwrap()
        .contains("往前数第 2 章"));

    let fresh = "陆照没再说话。院门外传来马蹄声，周砚舟的人举着火把进了山门。";
    assert!(ending_repetition_findings(&texts(&[earlier]), fresh).is_empty());
}

//! Deterministic, input-causal semantic compilation for the B11 entry flow.
//!
//! This module deliberately has no persistence or transport dependency.  It is
//! the bounded semantic boundary between a user's idea/market material and the
//! candidate artifacts assembled by `project_entry`.

use crate::generated::types::ArtifactRef;
use crate::hash::sha256_canonical;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticInput {
    pub input_kind: String,
    pub source_text: String,
    pub source_digest: String,
    pub market_source: Option<String>,
    pub platform: String,
    pub length: String,
    pub forbidden: String,
    pub preferences: Vec<String>,
    pub adjustment: String,
    pub generation: i64,
    pub created_at: String,
}

impl SemanticInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        input_kind: &str,
        source_text: &str,
        source_digest: &str,
        market_source: Option<&str>,
        platform: &str,
        length: &str,
        forbidden: &str,
        preferences: Vec<String>,
        adjustment: &str,
        generation: i64,
        created_at: &str,
    ) -> Self {
        Self {
            input_kind: input_kind.into(),
            source_text: source_text.into(),
            source_digest: source_digest.into(),
            market_source: market_source.map(ToOwned::to_owned),
            platform: platform.into(),
            length: length.into(),
            forbidden: forbidden.into(),
            preferences,
            adjustment: adjustment.into(),
            generation,
            created_at: created_at.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticDomain {
    Cultivation,
    NearFuture,
    UrbanRomance,
    Historical,
    Generic,
}

impl SemanticDomain {
    fn label(self) -> &'static str {
        match self {
            Self::Cultivation => "东方奇幻",
            Self::NearFuture => "近未来都市",
            Self::UrbanRomance => "都市情感",
            Self::Historical => "历史悬疑",
            Self::Generic => "原创长篇",
        }
    }
}

#[derive(Debug, Clone)]
struct SemanticProfile {
    domain: SemanticDomain,
    genre: &'static str,
    audience: &'static str,
    mechanism: String,
    conflict: String,
    desire: String,
    long_arc: String,
    focus: String,
    signals: Vec<&'static str>,
}

fn contains_any(text: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn infer_domain(text: &str) -> SemanticDomain {
    if contains_any(
        text,
        &["近未来", "未来", "科幻", "赛博", "算法", "记忆犯罪", "科技"],
    ) {
        SemanticDomain::NearFuture
    } else if contains_any(
        text,
        &[
            "都市情感",
            "女频",
            "恋爱",
            "情感",
            "婚姻",
            "职场",
            "总裁",
            "女性成长",
        ],
    ) {
        SemanticDomain::UrbanRomance
    } else if contains_any(
        text,
        &["历史", "古代", "宫廷", "侯府", "大明", "长安", "朝廷"],
    ) {
        SemanticDomain::Historical
    } else if contains_any(
        text,
        &[
            "修仙", "宗门", "妖兽", "炼丹", "废丹", "玄幻", "男频", "升级",
        ],
    ) {
        SemanticDomain::Cultivation
    } else {
        SemanticDomain::Generic
    }
}

fn short_focus(text: &str) -> String {
    let focus = text
        .split(['。', '，', ',', '.', ';', '；', ':', '：', '\n'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or("这份故事设想");
    focus.chars().take(32).collect()
}

fn profile_for(text: &str) -> SemanticProfile {
    let domain = infer_domain(text);
    match domain {
        SemanticDomain::Cultivation => SemanticProfile {
            domain,
            genre: "东方奇幻 · 成长悬疑",
            audience: "喜欢明确成长回报、规则代价和持续谜团的中文长篇读者",
            mechanism: "一种看似无用的修炼材料会改变被压制者的力量边界".into(),
            conflict: "掌握资源与规则解释权的宗门，试图封锁一项会改变秩序的秘密".into(),
            desire: "证明自己的价值，并保护不愿被当成资源的人".into(),
            long_arc: "从个人困境推进到宗门秩序、异族关系和世界规则来源".into(),
            focus: short_focus(text),
            signals: vec!["成长回报", "规则代价", "资源秩序"],
        },
        SemanticDomain::NearFuture => SemanticProfile {
            domain,
            genre: "近未来都市 · 记忆悬疑",
            audience: "喜欢科技悬疑、身份追查和城市阴影的泛悬疑读者",
            mechanism: "城市记忆证据会被算法篡改，而每次校验都会留下可追踪的异常痕迹".into(),
            conflict: "掌握城市档案的人可以重写公共记忆，调查者必须在真相与安全之间选择".into(),
            desire: "找回被抹去的真实经历，并阻止更多人失去自己的过去".into(),
            long_arc: "从一宗记忆犯罪扩展到城市档案权、群体记忆和真实身份的争夺".into(),
            focus: short_focus(text),
            signals: vec!["科技悬疑", "身份追查", "城市档案"],
        },
        SemanticDomain::UrbanRomance => SemanticProfile {
            domain,
            genre: "都市情感 · 关系成长",
            audience: "关注亲密关系、职业选择与女性成长的中文都市读者",
            mechanism: "一份关系契约会记录每次信任选择，并在关键时刻暴露双方真正的代价".into(),
            conflict: "个人成长与亲密关系的承诺彼此拉扯，现实利益又不断放大误解".into(),
            desire: "在不牺牲自我边界的前提下，建立值得信任的关系与生活".into(),
            long_arc: "从一段关系的试探推进到家庭、职业和自我选择的长期重建".into(),
            focus: short_focus(text),
            signals: vec!["关系修复", "自我边界", "现实选择"],
        },
        SemanticDomain::Historical => SemanticProfile {
            domain,
            genre: "历史悬疑 · 权力博弈",
            audience: "喜欢历史质感、身份谜案和制度冲突的中文长篇读者",
            mechanism: "朝廷档案会重写失势家族的身份，只有一份带代价的私人证词能够抵抗改写".into(),
            conflict: "官方记录与亲历者记忆互相冲突，主角必须决定公开真相要付出什么".into(),
            desire: "为家族找回被抹去的名字，同时不让新的权力继续复制旧案".into(),
            long_arc: "从一桩家族旧案推进到档案权、地方利益和制度修复的终局选择".into(),
            focus: short_focus(text),
            signals: vec!["历史质感", "身份旧案", "制度冲突"],
        },
        SemanticDomain::Generic => SemanticProfile {
            domain,
            genre: "原创长篇 · 类型悬疑",
            audience: "喜欢明确目标、人物选择和持续问题的中文长篇读者",
            mechanism: format!("“{}”会被一条可验证的行动规则改变", short_focus(text)),
            conflict: format!(
                "围绕“{}”的不同立场发生冲突，主角必须用行动而不是解释承担后果",
                short_focus(text)
            ),
            desire: format!(
                "为了{}，主角必须主动选择一次无法撤回的道路",
                short_focus(text)
            ),
            long_arc: format!(
                "从“{}”的个人问题扩展到关系、群体和世界规则的来源",
                short_focus(text)
            ),
            focus: short_focus(text),
            signals: vec!["人物选择", "行动后果", "持续问题"],
        },
    }
}

fn profile_for_input(input: &SemanticInput) -> SemanticProfile {
    let source_domain = infer_domain(&input.source_text);
    let adjustment_domain = infer_domain(&input.adjustment);
    if !input.adjustment.trim().is_empty() && adjustment_domain != SemanticDomain::Generic {
        profile_for(&input.adjustment)
    } else if source_domain != SemanticDomain::Generic {
        profile_for(&input.source_text)
    } else {
        let market_text = input.market_source.as_deref().unwrap_or_default();
        profile_for(&format!("{} {}", input.source_text, market_text))
    }
}

fn variant_label(index: usize) -> &'static str {
    match index {
        0 => "忠实展开",
        1 => "压力增强",
        _ => "视角偏转",
    }
}

fn adjusted_fields(
    input: &SemanticInput,
    profile: &SemanticProfile,
    index: usize,
) -> (String, String, String, String) {
    let mut mechanism = match index {
        0 => profile.mechanism.clone(),
        1 => format!(
            "{}；外部压力会在开篇变成必须回应的现实代价",
            profile.mechanism
        ),
        _ => format!(
            "{}；另一群体的证词会暴露这条规则的反常一面",
            profile.mechanism
        ),
    };
    let mut conflict = match index {
        0 => profile.conflict.clone(),
        1 => format!("{}；主角的第一次行动会立刻触发对手的反制", profile.conflict),
        _ => format!(
            "{}；同一冲突会在亲密关系与公共秩序之间形成镜像",
            profile.conflict
        ),
    };
    let mut desire = match index {
        0 => profile.desire.clone(),
        1 => format!("{}，并在首个关键节点主动夺回选择权", profile.desire),
        _ => format!("{}，同时确认谁有资格定义这段经历", profile.desire),
    };
    let mut long_arc = match index {
        0 => profile.long_arc.clone(),
        1 => format!("{}；每一卷都用更高代价检验主角的选择", profile.long_arc),
        _ => format!("{}；把同一问题交给不同立场的人共同回答", profile.long_arc),
    };
    let adjustment = input.adjustment.to_lowercase();
    if contains_any(&adjustment, &["人物关系", "关系", "同伴", "亲密"]) {
        mechanism.push_str("；关系中的信任变化会直接改变它的使用代价");
        conflict.push_str("；最重要的对抗发生在想保护彼此的人之间");
        desire.push_str("，并守住一段不被工具化的关系");
        long_arc.push_str("；关系修复成为推动真相公开的主线");
    }
    if contains_any(&adjustment, &["弱化升级", "不要升级", "减少升级"]) {
        mechanism.push_str("；进展主要来自信息与关系变化，而不是力量数值");
        conflict.push_str("；每次推进更强调信任、证据和选择的代价");
        long_arc.push_str("；后期回报优先兑现人物与关系，而不是单纯变强");
    }
    if contains_any(&adjustment, &["更主动", "主动", "主角主动"]) {
        desire.push_str("，不再等待别人替自己解释真相");
        conflict.push_str("；主角会主动设置一次反制局面");
    }
    (mechanism, conflict, desire, long_arc)
}

pub fn compile_pitch(
    input: &SemanticInput,
    index: usize,
    opportunity_ref: Option<&Value>,
) -> Value {
    let profile = profile_for_input(input);
    let (mechanism, conflict, desire, long_arc) = adjusted_fields(input, &profile, index);
    let name = format!("{}·{}", profile.domain.label(), variant_label(index));
    let platform = if input.platform.trim().is_empty() || input.platform == "AI 推荐" {
        "可按目标平台调整"
    } else {
        input.platform.as_str()
    };
    let promise = format!(
        "围绕{}，主角必须用行动承担代价；读者会持续看到{}。",
        desire, mechanism
    );
    let mut market_refs = Vec::new();
    if let Some(reference) = opportunity_ref {
        market_refs.push(reference.clone());
    }
    json!({
        "payloadSchema":"project-pitch.r1",
        "variant":variant_label(index),
        "name":name,
        "recommended":index == 0,
        "platform":platform,
        "genre":profile.genre,
        "audience":profile.audience,
        "promise":promise,
        "emotion":"从困境到主动选择的成长与关系回报",
        "reward":format!("每次推进都兑现一部分{}，同时打开更大的问题", profile.focus),
        "protagonistDesire":desire,
        "mechanism":mechanism,
        "worldConflict":conflict,
        "longArc":long_arc,
        "openingHook":format!("第一章出现与“{}”有关、且必须立即回应的异常。", profile.focus),
        "volumeGoal":format!("完成一次可验证的局部回报，并让“{}”进入主线", profile.focus),
        "expectedLength":if input.length.trim().is_empty() { "可按项目预算调整" } else { input.length.as_str() },
        "commercialEdge":"目标清楚、代价可见、每卷都有局部回报",
        "homogeneityRisk":"需要持续维护题材信号与人物选择的独特关系",
        "originality":format!("把{}与人物选择绑定为一条因果链", profile.signals.join("、")),
        "difficulty":"需要保持规则一致，同时让推进来自具体行动",
        "failureRisk":"如果只增加设定而不让主角选择，长期推进会减弱",
        "aiReason":"这是候选方向的解释，不替你做最终选择。",
        "ideaExcerpt":input.source_text.chars().take(280).collect::<String>(),
        "sourceInputDigest":input.source_digest,
        "inputKind":input.input_kind,
        "generation":input.generation,
        "createdAt":input.created_at,
        "userAdjustment":if input.adjustment.is_empty() { Value::Null } else { json!(input.adjustment) },
        "authoritative":false,
        "candidateStatus":"CANDIDATE",
        "marketEvidenceRefs":market_refs,
        "semanticProfile":{"domain":profile.domain.label(),"signals":profile.signals,"focus":profile.focus},
        "semanticInputs":{"sourceInputDigest":input.source_digest,"platform":input.platform,"length":input.length,"forbidden":input.forbidden,"preferences":input.preferences,"adjustment":input.adjustment},
        "generationProvenance":{"provider":"deterministic-recording","model":"b11-semantic-compiler-r1","compiler":"B11::StoryDirectionSemanticCompiler","modelRunRefs":[],"transportCalls":0,"sourceInputDigest":input.source_digest}
    })
}

pub fn compile_market_opportunity(input: &SemanticInput, index: usize) -> Value {
    let profile = profile_for_input(input);
    let (kind, angle) = match index {
        0 => ("稳健型", "清晰承诺"),
        1 => ("潜力型", "差异化冲突"),
        _ => ("差异化型", "长线记忆点"),
    };
    let title = format!("{}·{}机会", profile.domain.label(), angle);
    let direction = format!(
        "{} + {}",
        profile.signals[index % profile.signals.len()],
        profile.focus
    );
    let platform = if input.platform.trim().is_empty() || input.platform == "AI 推荐" {
        "待选择平台"
    } else {
        input.platform.as_str()
    };
    let base = match profile.domain {
        SemanticDomain::UrbanRomance => (82, 58, 84, 78, 86, 56, 80, 88, 78),
        SemanticDomain::NearFuture => (79, 61, 72, 86, 80, 68, 87, 84, 83),
        SemanticDomain::Historical => (72, 69, 70, 74, 76, 72, 82, 80, 75),
        SemanticDomain::Cultivation => (84, 76, 55, 78, 80, 60, 90, 74, 82),
        SemanticDomain::Generic => (74, 64, 68, 76, 74, 64, 80, 82, 76),
    };
    json!({
        "payloadSchema":"genre-opportunity.r1",
        "kind":kind,
        "title":title,
        "direction":direction,
        "platform":platform,
        "audience":profile.audience,
        "demand":base.0 - index as i64 * 2,
        "competition":base.1 + index as i64 * 3,
        "homogeneityRisk":base.2 + index as i64 * 2,
        "saturation":base.3 + index as i64,
        "growth":base.4 - index as i64 * 2,
        "monetization":base.5 + index as i64,
        "newcomerDifficulty":base.6 - index as i64,
        "longFormCapacity":base.7 - index as i64,
        "originalitySpace":base.8 + index as i64 * 2,
        "aiFeasibility":base.0 - index as i64,
        "complianceRisk":12 + index as i64 * 2,
        "dataConfidence":72,
        "recommendationConfidence":74 - index as i64 * 3,
        "evidence":["用户导入资料","可验证市场信号","仅作机会判断"],
        "recommendWhy":format!("资料中的{}信号与{}相符；这只是方向建议。", profile.signals.join("、"), profile.focus),
        "notRecommendWhy":"同类供给仍需持续观察，需要通过人物选择保持差异。",
        "source":input.market_source.as_deref().unwrap_or("用户导入资料"),
        "collectedAt":input.created_at,
        "policyVersion":"B11-USER-MATERIAL-1",
        "semanticProfile":{"domain":profile.domain.label(),"signals":profile.signals,"focus":profile.focus},
        "provenance":{"source":input.market_source.as_deref().unwrap_or("用户导入资料"),"capturedAt":input.created_at,"scope":"MARKET","inputDigest":input.source_digest,"kind":"USER_PROVIDED_DATA"}
    })
}

pub fn merge_candidates(
    selected_ids: &[String],
    selected: &[Value],
    selected_refs: &[ArtifactRef],
    now: &str,
) -> Result<Value, String> {
    if selected.is_empty() || selected.len() != selected_ids.len() {
        return Err("B11_SELECTION_REQUIRED".into());
    }
    let field = |name: &str| {
        selected
            .iter()
            .filter_map(|item| item.get(name).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("；")
    };
    let contributions = selected
        .iter()
        .zip(selected_ids.iter())
        .map(|(item, id)| {
            json!({
                "pitchId":id,
                "variant":item["variant"],
                "mechanism":item["mechanism"],
                "worldConflict":item["worldConflict"],
                "protagonistDesire":item["protagonistDesire"],
                "longArc":item["longArc"]
            })
        })
        .collect::<Vec<_>>();
    let source_digest = sha256_canonical(&json!({
        "selectedIds":selected_ids,
        "sourceDigests":selected.iter().map(|item| item["sourceInputDigest"].clone()).collect::<Vec<_>>()
    }))
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "payloadSchema":"story-blueprint.r1",
        "selectedPitchIds":selected_ids,
        "selectedPitchRefs":selected_refs,
        "sourceInputDigest":source_digest,
        "storyPromise":field("promise"),
        "protagonistDirection":field("protagonistDesire"),
        "mechanismBoundary":field("mechanism"),
        "coreConflict":field("worldConflict"),
        "readerPromise":field("promise"),
        "longTermDirection":field("longArc"),
        "semanticContributions":contributions,
        "explicitUserDecision":true,
        "authoritative":true,
        "createdAt":now
    }))
}

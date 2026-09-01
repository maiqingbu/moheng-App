//! B11 Project Entry owner.
//!
//! This module turns a bounded user idea or user-provided market document into
//! non-authoritative candidates, then records explicit user decisions and the
//! minimum opening-ready contracts. Durable writes still go through the frozen
//! CommitService; this module never writes SQL or becomes a second production
//! engine.

use crate::commit::CommitService;
use crate::generated::types::{ArtifactEnvelope, ArtifactRef, CommitBundle};
use crate::hash::sha256_canonical;
use crate::planning;
use crate::project_db::ProjectHandle;
use crate::repositories::project_entry::{self, EntryArtifactRow, OpeningTopology};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Map, Value};

const MAX_IDEA_CHARS: usize = 8_000;
const MAX_MARKET_MATERIAL_CHARS: usize = 12_000;
const PITCH_COUNT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Idea,
    Market,
}

impl EntryKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idea => "IDEA",
            Self::Market => "MARKET",
        }
    }
}

fn required_text(value: &Value, path: &str) -> Result<String, String> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("B11_INVALID:{path}"))
}

fn bounded_text(value: &str, max_chars: usize) -> Result<String, String> {
    if value.chars().count() > max_chars {
        return Err("B11_INPUT_TOO_LARGE".into());
    }
    Ok(value.to_owned())
}

fn project_scope() -> Value {
    json!({"level":"PROJECT"})
}

fn market_scope() -> Value {
    json!({"level":"MARKET"})
}

fn chapter_scope(chapter_id: &str) -> Value {
    json!({"level":"CHAPTER","chapterId":chapter_id})
}

fn artifact_ref(artifact: &ArtifactEnvelope) -> ArtifactRef {
    ArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        version: artifact.version,
        payload_hash: artifact.payload_hash.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn artifact(
    project_id: &str,
    artifact_id: String,
    artifact_type: &str,
    scope: Value,
    payload: Value,
    dependencies: Vec<ArtifactRef>,
    created_by: &str,
    command_id: &str,
    now: &str,
) -> Result<ArtifactEnvelope, String> {
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":artifact_type,
        "scope":scope,
        "version":1,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"PROPOSAL",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":dependencies,
        "provenance":{"createdBy":created_by,"commandId":command_id},
        "createdAt":now
    }))
    .map_err(|error| format!("B11_ARTIFACT_INVALID:{error}"))
}

#[allow(clippy::too_many_arguments)]
fn bundle(
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: Option<i64>,
    now: &str,
    artifacts: Vec<ArtifactEnvelope>,
    heads: Option<Vec<Value>>,
    decisions: Option<Vec<Value>>,
    domain: &str,
    receipt_payload: Map<String, Value>,
) -> Result<CommitBundle, String> {
    let refs = artifacts.iter().map(artifact_ref).collect::<Vec<_>>();
    let mut value = json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":now,
        "artifacts":artifacts,
        "receiptDraft":{"domain":domain,"artifactRefs":refs,"payload":receipt_payload}
    });
    if let Some(heads) = heads {
        value["headUpdates"] = Value::Array(heads);
    }
    if let Some(decisions) = decisions {
        value["decisionUpdates"] = Value::Array(decisions);
    }
    serde_json::from_value(value).map_err(|error| format!("B11_BUNDLE_INVALID:{error}"))
}

fn receipt_value(receipt: crate::generated::types::CommitReceipt) -> Result<Value, String> {
    serde_json::to_value(receipt).map_err(|error| error.to_string())
}

fn pitch_templates() -> [(&'static str, &'static str, &'static str, &'static str); PITCH_COUNT] {
    [
        (
            "原始想法",
            "废丹让妖兽进化",
            "被逐出宗门的炼丹师必须证明废丹的价值，同时阻止宗门封锁真相",
            "证明自己并找回师门尊严",
        ),
        (
            "商业增强",
            "妖兽学会炼丹",
            "边境妖兽开始使用人类丹术，主角必须在宗门清剿与跨族谈判之间做选择",
            "救下将被当作材料的妖兽群",
        ),
        (
            "差异化",
            "废丹交换记忆",
            "每一颗废丹都能换回一段被抹去的记忆，但真相会反过来动摇主角的身份",
            "找回失踪亲人的最后一段记忆",
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn pitch_payload(
    kind: EntryKind,
    variant: &str,
    index: usize,
    idea: &str,
    source_digest: &str,
    generation: i64,
    now: &str,
    opportunity_ref: Option<&ArtifactRef>,
) -> Value {
    let templates = pitch_templates();
    let (_, mechanism, conflict, desire) = templates[index];
    let name = match index {
        0 => "废丹成潮",
        1 => "边境丹契",
        _ => "记忆炼成灰",
    };
    let platform = if kind == EntryKind::Market {
        "起点中文网"
    } else {
        "起点中文网 / 可调整"
    };
    let mut market_refs = Vec::new();
    if let Some(reference) = opportunity_ref {
        market_refs.push(json!(reference));
    }
    json!({
        "payloadSchema":"project-pitch.r1",
        "variant":variant,
        "name":name,
        "recommended":index == 0,
        "platform":platform,
        "genre":"东方奇幻 · 悬疑成长",
        "audience":"喜欢强目标、升级回报和身份谜局的中文长篇读者",
        "promise":format!("{mechanism}；每次推进都必须付出可验证的代价。"),
        "emotion":"从受挫到主动破局的成长回报",
        "reward":"不断揭开机制真相，同时让主角在选择中获得新的能力边界",
        "protagonistDesire":desire,
        "mechanism":mechanism,
        "worldConflict":conflict,
        "longArc":"从宗门旧案扩展到妖兽文明、记忆交易与世界规则的来源",
        "openingHook":format!("第一章出现与“{mechanism}”有关的异常结果。"),
        "volumeGoal":"建立机制代价，完成第一次局部回报，并留下更大的身份问题",
        "expectedLength":"120–180 万字",
        "commercialEdge":"强目标开局、明确升级代价、每卷都有可兑现的小回报",
        "homogeneityRisk":"传统宗门成长题材供给较高，需持续保持机制与人物选择的差异",
        "originality":"把炼丹、妖兽和身份谜局绑定为同一条因果链",
        "difficulty":"需要保持规则一致，同时让升级不只依赖数值变强",
        "failureRisk":"若只堆设定而缺少主角主动选择，长期推进会变慢",
        "aiReason":"这是候选方向的解释，不替你做最终选择。",
        "ideaExcerpt":idea.chars().take(280).collect::<String>(),
        "sourceInputDigest":source_digest,
        "inputKind":kind.as_str(),
        "generation":generation,
        "createdAt":now,
        "authoritative":false,
        "candidateStatus":"CANDIDATE",
        "marketEvidenceRefs":market_refs,
        "generationProvenance":{"provider":"deterministic-recording","model":"b11-pitch-v1","modelRunRefs":[],"transportCalls":0,"sourceInputDigest":source_digest}
    })
}

fn market_opportunity_payload(
    index: usize,
    source: &str,
    material_digest: &str,
    now: &str,
) -> Value {
    let templates = [
        ("稳健型", "强目标东方奇幻", "强目标 + 规则悬疑"),
        ("潜力型", "人与妖兽共生成长", "跨族关系 + 成长"),
        ("差异化型", "记忆机制悬疑长篇", "身份谜局 + 记忆代价"),
    ];
    let (kind, title, direction) = templates[index];
    json!({
        "payloadSchema":"genre-opportunity.r1",
        "kind":kind,
        "title":title,
        "direction":direction,
        "platform":"起点中文网",
        "audience":"偏好东方奇幻、升级回报和持续谜团的长篇读者",
        "demand":78 - index as i64 * 4,
        "competition":64 + index as i64 * 5,
        "homogeneityRisk":52 + index as i64 * 4,
        "saturation":58 + index as i64 * 3,
        "growth":82 - index as i64 * 2,
        "monetization":75 - index as i64,
        "newcomerDifficulty":61 + index as i64 * 5,
        "longFormCapacity":88 - index as i64 * 3,
        "originalitySpace":76 + index as i64 * 3,
        "aiFeasibility":81 - index as i64 * 2,
        "complianceRisk":12 + index as i64 * 2,
        "dataConfidence":72,
        "recommendationConfidence":74 - index as i64 * 3,
        "evidence":["用户导入资料","可验证开篇信号","仅作机会判断"],
        "recommendWhy":"资料显示强目标、明确代价和首章异常更容易形成继续阅读理由；这只是方向建议。",
        "notRecommendWhy":"同类供给不低，需要通过机制代价和人物选择保持差异。",
        "source":source,
        "collectedAt":now,
        "policyVersion":"B11-USER-MATERIAL-1",
        "provenance":{"source":source,"capturedAt":now,"scope":"MARKET","inputDigest":material_digest,"kind":"USER_PROVIDED_DATA"}
    })
}

fn initial_input(request: &Value, kind: EntryKind) -> Result<(String, String, String), String> {
    let payload = request.get("payload").unwrap_or(&Value::Null);
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未命名作品")
        .to_owned();
    match kind {
        EntryKind::Idea => {
            let idea = bounded_text(
                payload
                    .get("idea")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("新的小说计划"),
                MAX_IDEA_CHARS,
            )?;
            let digest = sha256_canonical(&json!({"kind":"IDEA","idea":idea}))
                .map_err(|error| error.to_string())?;
            Ok((title, idea, digest))
        }
        EntryKind::Market => {
            let _source = payload
                .get("marketSource")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "B11_MARKET_SOURCE_REQUIRED".to_string())?;
            let material = bounded_text(
                &required_text(payload, "/marketMaterial")?,
                MAX_MARKET_MATERIAL_CHARS,
            )?;
            Ok((
                title,
                material.clone(),
                sha256_canonical(&json!({"kind":"MARKET","material":material}))
                    .map_err(|error| error.to_string())?,
            ))
        }
    }
}

pub fn validate_initial_request(request: &Value, kind: EntryKind) -> Result<(), String> {
    initial_input(request, kind).map(|_| ())
}

pub fn commit_initial(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    now: &str,
    request: &Value,
    kind: EntryKind,
) -> Result<crate::generated::types::CommitReceipt, String> {
    let (title, input, source_digest) = initial_input(request, kind)?;
    let brief_id = format!("brief:{project_id}:{command_id}");
    let brief_payload = if kind == EntryKind::Idea {
        json!({
            "payloadSchema":"creative-brief.r1","inputKind":"IDEA","title":title,
            "idea":input,"sourceInputDigest":source_digest,"authoritative":false,"createdAt":now
        })
    } else {
        let source = required_text(
            request.get("payload").unwrap_or(&Value::Null),
            "/marketSource",
        )?;
        json!({
            "payloadSchema":"creative-brief.r1","inputKind":"MARKET","title":title,
            "marketMaterial":input,"marketSource":source,"sourceInputDigest":source_digest,
            "authoritative":false,"createdAt":now
        })
    };
    let brief = artifact(
        project_id,
        brief_id,
        "CreativeBrief",
        project_scope(),
        brief_payload,
        Vec::new(),
        "B11::ProjectEntryOwner",
        command_id,
        now,
    )
    .map_err(|error| error.to_string())?;
    let brief_ref = artifact_ref(&brief);
    let mut artifacts = vec![brief];
    let mut opportunity_refs = Vec::new();
    if kind == EntryKind::Market {
        let payload = request.get("payload").unwrap_or(&Value::Null);
        let source = required_text(payload, "/marketSource")?;
        let snapshot_id = format!("market-snapshot:{project_id}:{command_id}");
        let snapshot = artifact(
            project_id,
            snapshot_id,
            "MarketSnapshot",
            market_scope(),
            json!({
                "payloadSchema":"market-snapshot.r1","source":source,"capturedAt":now,
                "scope":"MARKET","materialDigest":source_digest,"material":input,
                "provenance":{"source":source,"capturedAt":now,"inputDigest":source_digest,"kind":"USER_PROVIDED_DATA"},
                "externalTextClassification":"DATA_ONLY"
            }),
            vec![brief_ref.clone()],
            "B11::MarketEvidenceOwner",
            command_id,
            now,
        )?;
        let snapshot_ref = artifact_ref(&snapshot);
        let evidence = artifact(
            project_id,
            format!("market-evidence:{project_id}:{command_id}"),
            "MarketEvidenceBundle",
            market_scope(),
            json!({
                "payloadSchema":"market-evidence-bundle.r1","source":source,"capturedAt":now,
                "scope":"MARKET","inputDigest":source_digest,
                "evidence":[{"evidenceId":"market-evidence:1","excerpt":input.chars().take(500).collect::<String>(),"classification":"DATA_ONLY"}],
                "instructionIsolation":{"externalText":"DATA_ONLY","interpretedAsInstruction":false},
                "provenance":{"source":source,"capturedAt":now,"inputDigest":source_digest}
            }),
            vec![snapshot_ref],
            "B11::MarketEvidenceOwner",
            command_id,
            now,
        )?;
        let evidence_ref = artifact_ref(&evidence);
        artifacts.push(snapshot);
        artifacts.push(evidence);
        for index in 0..PITCH_COUNT {
            let opportunity = artifact(
                project_id,
                format!("opportunity:{project_id}:{command_id}:{}", index + 1),
                "GenreOpportunity",
                market_scope(),
                market_opportunity_payload(index, source.as_str(), &source_digest, now),
                vec![evidence_ref.clone()],
                "B11::MarketEvidenceOwner",
                command_id,
                now,
            )?;
            opportunity_refs.push(artifact_ref(&opportunity));
            artifacts.push(opportunity);
        }
    }
    for index in 0..PITCH_COUNT {
        let (variant, _, _, _) = pitch_templates()[index];
        let pitch = artifact(
            project_id,
            format!("pitch:{project_id}:{command_id}:{}", index + 1),
            "ProjectPitch",
            project_scope(),
            pitch_payload(
                kind,
                variant,
                index,
                &input,
                &source_digest,
                1,
                now,
                opportunity_refs.get(index),
            ),
            if let Some(reference) = opportunity_refs.get(index) {
                vec![brief_ref.clone(), reference.clone()]
            } else {
                vec![brief_ref.clone()]
            },
            "B11::ProjectEntryOwner",
            command_id,
            now,
        )?;
        artifacts.push(pitch);
    }
    let receipt_payload = json!({
        "entryKind":kind.as_str(),"candidateCount":PITCH_COUNT,"sourceInputDigest":source_digest,
        "explicitSelectionRequired":true,"authoritativeSelection":false,"marketEvidenceIsDataOnly":kind == EntryKind::Market
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    let commit = bundle(
        project_id,
        command_id,
        idempotency_key,
        Some(0),
        now,
        artifacts,
        None,
        None,
        "B11_PROJECT_ENTRY",
        receipt_payload,
    )?;
    CommitService::commit_without_revision(connection, &commit, now)
        .map_err(|error| error.to_string())
}

pub fn commit_regenerate(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<crate::generated::types::CommitReceipt, String> {
    let brief = project_entry::latest_artifact_by_type(connection, project_id, "CreativeBrief")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B11_IDEA_ENTRY_REQUIRED".to_string())?;
    let source_input_digest = brief
        .payload
        .get("sourceInputDigest")
        .and_then(Value::as_str)
        .ok_or_else(|| "B11_SOURCE_DIGEST_MISSING".to_string())?;
    let idea = brief
        .payload
        .get("idea")
        .or_else(|| brief.payload.get("marketMaterial"))
        .and_then(Value::as_str)
        .unwrap_or("新的故事方向")
        .to_owned();
    let generation = project_entry::latest_generation(connection, project_id, "ProjectPitch")
        .map_err(|error| error.to_string())?
        .unwrap_or(0)
        + 1;
    let adjustment = request
        .pointer("/payload/adjustment")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let mut artifacts = Vec::new();
    let brief_ref = ArtifactRef {
        artifact_id: brief.artifact_id.clone(),
        version: brief.version,
        payload_hash: brief.payload_hash.clone(),
    };
    for index in 0..PITCH_COUNT {
        let (variant, _, _, _) = pitch_templates()[index];
        let mut payload = pitch_payload(
            if brief.payload.get("inputKind").and_then(Value::as_str) == Some("MARKET") {
                EntryKind::Market
            } else {
                EntryKind::Idea
            },
            variant,
            index,
            &idea,
            source_input_digest,
            generation,
            now,
            None,
        );
        if !adjustment.is_empty() {
            payload["userAdjustment"] = json!(adjustment);
        }
        artifacts.push(artifact(
            project_id,
            format!("pitch:{project_id}:{command_id}:{}", index + 1),
            "ProjectPitch",
            project_scope(),
            payload,
            vec![brief_ref.clone()],
            "B11::ProjectEntryOwner",
            command_id,
            now,
        )?);
    }
    let receipt_payload = json!({"generation":generation,"candidateCount":PITCH_COUNT,"explicitSelectionRequired":true})
        .as_object()
        .cloned()
        .unwrap_or_default();
    let commit = bundle(
        project_id,
        command_id,
        idempotency_key,
        Some(expected_revision),
        now,
        artifacts,
        None,
        None,
        "B11_PITCH_REGENERATION",
        receipt_payload,
    )?;
    CommitService::commit(connection, &commit, now).map_err(|error| error.to_string())
}

pub fn commit_selection(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<crate::generated::types::CommitReceipt, String> {
    let selected_ids = request
        .pointer("/payload/pitchIds")
        .and_then(Value::as_array)
        .ok_or_else(|| "B11_SELECTION_REQUIRED".to_string())?
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if selected_ids.is_empty() || selected_ids.len() > 2 {
        return Err("B11_SELECTION_COUNT_INVALID".into());
    }
    let unique_ids = selected_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if unique_ids.len() != selected_ids.len() {
        return Err("B11_SELECTION_DUPLICATE".into());
    }
    let mut selected = Vec::new();
    for id in &selected_ids {
        let row = project_entry::artifact_by_id(connection, project_id, id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "B11_PITCH_NOT_FOUND".to_string())?;
        if row.artifact_type != "ProjectPitch" || row.artifact_status != "COMMITTED" {
            return Err("B11_PITCH_NOT_SELECTABLE".into());
        }
        selected.push(row);
    }
    if selected
        .iter()
        .map(|row| row.payload["generation"].clone())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0] != pair[1])
    {
        return Err("B11_PITCH_GENERATION_MISMATCH".into());
    }
    let selected_refs = selected
        .iter()
        .map(|row| ArtifactRef {
            artifact_id: row.artifact_id.clone(),
            version: row.version,
            payload_hash: row.payload_hash.clone(),
        })
        .collect::<Vec<_>>();
    let first = &selected[0].payload;
    let blueprint = artifact(
        project_id,
        format!("story-blueprint:{project_id}:{command_id}"),
        "StoryBlueprint",
        project_scope(),
        json!({
            "payloadSchema":"story-blueprint.r1","selectedPitchIds":selected_ids,
            "selectedPitchRefs":selected_refs,"sourceInputDigest":first["sourceInputDigest"],
            "storyPromise":first["promise"],"protagonistDirection":first["protagonistDesire"],
            "mechanismBoundary":first["mechanism"],"coreConflict":first["worldConflict"],
            "readerPromise":first["promise"],"longTermDirection":first["longArc"],
            "explicitUserDecision":true,"authoritative":true,"createdAt":now
        }),
        selected_refs.clone(),
        "B11::ProjectSelectionOwner",
        command_id,
        now,
    )?;
    let blueprint_ref = artifact_ref(&blueprint);
    let decision = json!({
        "decisionId":format!("decision:{project_id}:{command_id}"),
        "decisionType":"STORY_DIRECTION_SELECTION","outcome":"SELECTED",
        "reason":"用户明确采用故事方向","sourceRef":blueprint_ref.artifact_id,
        "payloadJson":{"selectedPitchRefs":selected_refs.clone(),"explicitUserDecision":true}
    });
    let receipt_payload = json!({"selectedPitchCount":selected_refs.len(),"explicitUserDecision":true,"canonicalHead":"StoryBlueprint"})
        .as_object().cloned().unwrap_or_default();
    let head = json!({"artifactType":"StoryBlueprint","scope":project_scope(),"artifactId":blueprint_ref.artifact_id,"version":blueprint_ref.version,"payloadHash":blueprint_ref.payload_hash});
    let commit = bundle(
        project_id,
        command_id,
        idempotency_key,
        Some(expected_revision),
        now,
        vec![blueprint],
        Some(vec![head]),
        Some(vec![decision]),
        "B11_PITCH_SELECTION",
        receipt_payload,
    )?;
    CommitService::commit(connection, &commit, now).map_err(|error| error.to_string())
}

fn story_core_from_request(request: &Value, blueprint: &EntryArtifactRow) -> Value {
    let supplied = request
        .pointer("/payload/storyCore")
        .cloned()
        .unwrap_or(Value::Null);
    if supplied.is_object() {
        return supplied;
    }
    json!({
        "protagonist":blueprint.payload["protagonistDirection"],
        "desire":blueprint.payload["protagonistDirection"],
        "mechanism":blueprint.payload["mechanismBoundary"],
        "conflict":blueprint.payload["coreConflict"],
        "worldRules":["核心机制必须支付可验证代价"],
        "readerPromise":blueprint.payload["readerPromise"],
        "forbiddenReveals":["前三章不揭开机制来源"],
        "style":"紧凑、悬疑、带成长回报",
        "expandableDirections":["首卷局部回收","多卷机制真相"]
    })
}

pub fn commit_story_setup(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<crate::generated::types::CommitReceipt, String> {
    let blueprint = project_entry::canonical_head(connection, project_id, "StoryBlueprint")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B11_SELECTION_REQUIRED".to_string())?;
    let blueprint_ref = ArtifactRef {
        artifact_id: blueprint.artifact_id.clone(),
        version: blueprint.version,
        payload_hash: blueprint.payload_hash.clone(),
    };
    let core = story_core_from_request(request, &blueprint);
    let story_promise = required_text(&core, "/readerPromise")?;
    let protagonist = required_text(&core, "/protagonist")?;
    let constitution = artifact(
        project_id,
        format!("story-constitution:{project_id}:{command_id}"),
        "StoryConstitution",
        project_scope(),
        json!({"payloadSchema":"story-constitution.r1","storyCore":core,"storyPromise":story_promise,"protagonist":protagonist,"selectedBlueprintRef":blueprint_ref,"minimumOpeningBoundary":true,"generatedFutureChapterPlans":0,"authoritative":true,"createdAt":now}),
        vec![blueprint_ref.clone()],
        "B11::StorySetupOwner",
        command_id,
        now,
    )?;
    let constitution_ref = artifact_ref(&constitution);
    let main_contract = artifact(
        project_id,
        format!("main-contract:{project_id}:{command_id}"),
        "MainContract",
        project_scope(),
        json!({"payloadSchema":"main-contract.r1","storyConstitutionRef":constitution_ref,"scope":"BOOK","hardRules":["主角必须在关键节点主动选择","核心机制的代价不可被跳过"],"softPreferences":[],"createdAt":now}),
        vec![constitution_ref.clone()],
        "B11::StorySetupOwner",
        command_id,
        now,
    )?;
    let main_ref = artifact_ref(&main_contract);
    let opening_contract = artifact(
        project_id,
        format!("opening-contract:{project_id}:{command_id}"),
        "OpeningContract",
        project_scope(),
        json!({"payloadSchema":"opening-contract.r1","storyConstitutionRef":constitution_ref,"mainContractRef":main_ref,"openingReady":false,"firstChapterGoal":"让主角面对一个必须主动回应的异常","firstThreeChapterPromise":"兑现一次局部机制回报并留下更深的问题","forbiddenReveals":core["forbiddenReveals"],"createdAt":now}),
        vec![constitution_ref.clone(), main_ref.clone()],
        "B11::StorySetupOwner",
        command_id,
        now,
    )?;
    let opening_ref = artifact_ref(&opening_contract);
    let book_plan = artifact(
        project_id,
        format!("book-plan-seed:{project_id}:{command_id}"),
        "BookPlan",
        project_scope(),
        json!({"payloadSchema":"book-plan.r1","planningMaturity":"SEED","storyPromise":story_promise,"protagonistMacroDirection":protagonist,"endingBoundary":"解决核心机制与主角选择之间的权力冲突","mustNotBreak":[{"id":"rule:mechanism-cost","description":"核心机制每次使用都必须产生可验证代价"}],"openingHypotheses":["开篇先展示异常结果，再逐步解释规则"],"globalLengthBudget":{"preferredWords":1200000},"createdAt":now}),
        vec![constitution_ref.clone(), opening_ref.clone()],
        "B07::PlanningOwner",
        command_id,
        now,
    )?;
    let book_ref = artifact_ref(&book_plan);
    let heads = vec![
        json!({"artifactType":"StoryConstitution","scope":project_scope(),"artifactId":constitution_ref.artifact_id,"version":constitution_ref.version,"payloadHash":constitution_ref.payload_hash}),
        json!({"artifactType":"MainContract","scope":project_scope(),"artifactId":main_ref.artifact_id,"version":main_ref.version,"payloadHash":main_ref.payload_hash}),
        json!({"artifactType":"OpeningContract","scope":project_scope(),"artifactId":opening_ref.artifact_id,"version":opening_ref.version,"payloadHash":opening_ref.payload_hash}),
        json!({"artifactType":"BookPlan","scope":project_scope(),"artifactId":book_ref.artifact_id,"version":book_ref.version,"payloadHash":book_ref.payload_hash}),
    ];
    let decision = json!({"decisionId":format!("decision:{project_id}:{command_id}"),"decisionType":"STORY_CORE_CONFIRMATION","outcome":"APPROVED","reason":"用户保存并确认故事核心","sourceRef":constitution_ref.artifact_id,"payloadJson":{"explicitUserConfirmation":true}});
    let commit = bundle(
        project_id,
        command_id,
        idempotency_key,
        Some(expected_revision),
        now,
        vec![constitution, main_contract, opening_contract, book_plan],
        Some(heads),
        Some(vec![decision]),
        "B11_STORY_SETUP",
        json!({"minimumBoundary":true,"generatedFutureChapterPlans":0,"explicitUserConfirmation":true}).as_object().cloned().unwrap_or_default(),
    )?;
    CommitService::commit(connection, &commit, now).map_err(|error| error.to_string())
}

fn opening_ids(project_id: &str) -> Result<(String, String, String, String), String> {
    Ok((
        StableIdentityFactory::from_command(StableIdKind::Book, &format!("{project_id}:book"))?,
        StableIdentityFactory::from_command(StableIdKind::Volume, &format!("{project_id}:volume"))?,
        StableIdentityFactory::from_command(
            StableIdKind::Chapter,
            &format!("{project_id}:chapter"),
        )?,
        format!("budget:{project_id}:opening"),
    ))
}

fn prepare_plan_commit(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    topology: &OpeningTopology,
) -> Result<crate::generated::types::CommitReceipt, String> {
    let constitution =
        project_entry::canonical_head_ref(connection, project_id, "StoryConstitution")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "B11_STORY_SETUP_REQUIRED".to_string())?;
    let book_plan = project_entry::canonical_head_ref(connection, project_id, "BookPlan")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B11_STORY_SETUP_REQUIRED".to_string())?;
    let state_digest = sha256_canonical(&json!({"projectId":project_id,"constitution":constitution,"bookPlan":book_plan,"opening":true}))
        .map_err(|error| error.to_string())?;
    let context_payload = json!({
        "payloadSchema":"context-manifest.r1","executionMode":"OPENING_BOOTSTRAP",
        "compiledFromProjectRevision":expected_revision + 1,"compiledFromStateDigest":state_digest,
        "goalContractRefs":[constitution.clone(),book_plan.clone()],"activeVolumeTaskRefs":[],"continuityThreadRefs":[],"futureReservationRefs":[],
        "narrativeStateSources":[{"kind":"StoryConstitution","ref":constitution.artifact_id}],"selectedEvidence":[],"hardSourceCount":2,"omittedSoftSources":[],"budget":{"hardTokens":1024,"softTokens":0,"totalTokens":1024}
    });
    let context = artifact(
        project_id,
        format!("context:{project_id}:{command_id}"),
        "ContextManifest",
        chapter_scope(&topology.chapter_id),
        context_payload,
        vec![constitution.clone(), book_plan.clone()],
        "B06::ContextCompilerOwner",
        command_id,
        now,
    )?;
    let context_ref = artifact_ref(&context);
    let chapter_plan_payload = json!({
        "payloadSchema":"chapter-plan.r1","chapterId":topology.chapter_id,"compiledFromProjectRevision":expected_revision + 1,"compiledFromStateDigest":state_digest,
        "contextManifestRef":context_ref,"chapterGoal":"让主角面对一个必须主动回应的异常","activeTaskRefs":[],"continuityActions":[],"requiredStateOutcomes":[],"forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":[],"sceneExecution":[],
        "horizonPreview":[{"slotNo":1,"objective":"展示异常并迫使主角选择","tentative":true},{"slotNo":2,"objective":"给出局部回报并留下代价","tentative":true},{"slotNo":3,"objective":"把问题推向首卷主线","tentative":true}]
    });
    planning::validate_current_chapter_plan(&chapter_plan_payload, &topology.chapter_id, &[], &[])?;
    let chapter_plan = artifact(
        project_id,
        format!("chapter-plan:{project_id}:{command_id}"),
        "ChapterPlan",
        chapter_scope(&topology.chapter_id),
        chapter_plan_payload,
        vec![context_ref.clone()],
        "B07::PlanningOwner",
        command_id,
        now,
    )?;
    let plan_ref = artifact_ref(&chapter_plan);
    let heads = vec![
        json!({"artifactType":"ChapterPlan","scope":chapter_scope(&topology.chapter_id),"artifactId":plan_ref.artifact_id,"version":plan_ref.version,"payloadHash":plan_ref.payload_hash}),
    ];
    let commit = bundle(
        project_id,
        command_id,
        idempotency_key,
        Some(expected_revision),
        now,
        vec![context, chapter_plan],
        Some(heads),
        None,
        "B11_OPENING_READY",
        json!({"chapterId":topology.chapter_id,"owner":"B07::PlanningOwner","minimumBoundary":true}).as_object().cloned().unwrap_or_default(),
    )?;
    CommitService::commit(connection, &commit, now).map_err(|error| error.to_string())
}

pub fn commit_opening_ready(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<crate::generated::types::CommitReceipt, String> {
    if request
        .pointer("/payload/approved")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("B11_OPENING_APPROVAL_REQUIRED".into());
    }
    let constitution =
        project_entry::canonical_head_ref(connection, project_id, "StoryConstitution")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "B11_STORY_SETUP_REQUIRED".to_string())?;
    let book_plan = project_entry::canonical_head_ref(connection, project_id, "BookPlan")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B11_STORY_SETUP_REQUIRED".to_string())?;
    let topology_ids = opening_ids(project_id)?;
    let topology = project_entry::ensure_opening_topology(
        connection,
        project_id,
        &topology_ids.0,
        &topology_ids.1,
        &topology_ids.2,
        &topology_ids.3,
        "开篇种子",
        now,
    )
    .map_err(|error| error.to_string())?;
    let opening_command_id = format!("{command_id}:opening-contract");
    let opening = artifact(
        project_id,
        format!("opening-contract:{project_id}:{command_id}"),
        "OpeningContract",
        project_scope(),
        json!({"payloadSchema":"opening-contract.r1","storyConstitutionRef":constitution,"bookPlanRef":book_plan,"openingReady":true,"firstChapterGoal":"让主角面对一个必须主动回应的异常","firstThreeChapterPromise":"兑现一次局部机制回报并留下更深的问题","forbiddenReveals":["前三章不揭开机制来源"],"createdAt":now}),
        vec![constitution, book_plan],
        "B11::OpeningPreparationOwner",
        &opening_command_id,
        now,
    )?;
    let opening_ref = artifact_ref(&opening);
    let gate_policy = artifact(
        project_id,
        format!("gate-policy:{project_id}:{command_id}"),
        "GatePolicy",
        project_scope(),
        json!({"payloadSchema":"gate-policy.r1","policyId":"b11-opening","rules":["story-core-bound","opening-ready","existing-b08-production-root"],"createdAt":now}),
        vec![opening_ref.clone()],
        "B11::OpeningPreparationOwner",
        &opening_command_id,
        now,
    )?;
    let gate_ref = artifact_ref(&gate_policy);
    let first_commit = bundle(
        project_id,
        &opening_command_id,
        &format!("{idempotency_key}:opening-contract"),
        Some(expected_revision),
        now,
        vec![opening, gate_policy],
        Some(vec![
            json!({"artifactType":"OpeningContract","scope":project_scope(),"artifactId":opening_ref.artifact_id,"version":opening_ref.version,"payloadHash":opening_ref.payload_hash}),
            json!({"artifactType":"GatePolicy","scope":project_scope(),"artifactId":gate_ref.artifact_id,"version":gate_ref.version,"payloadHash":gate_ref.payload_hash}),
        ]),
        None,
        "B11_OPENING_READY",
        json!({"openingReady":true,"chapterId":topology.chapter_id})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )?;
    let first_receipt =
        CommitService::commit(connection, &first_commit, now).map_err(|error| error.to_string())?;
    prepare_plan_commit(
        connection,
        project_id,
        &format!("{command_id}:plan"),
        &format!("{idempotency_key}:plan"),
        first_receipt.after_revision,
        now,
        &topology,
    )
}

pub fn start_creating(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> Result<Value, String> {
    let opening = project_entry::canonical_head(connection, project_id, "OpeningContract")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B11_OPENING_NOT_READY".to_string())?;
    if opening.payload.get("openingReady").and_then(Value::as_bool) != Some(true) {
        return Err("B11_OPENING_NOT_READY".into());
    }
    let topology = project_entry::opening_topology(connection, project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B11_OPENING_TOPOLOGY_MISSING".to_string())?;
    let plan = project_entry::canonical_chapter_head(
        connection,
        project_id,
        "ChapterPlan",
        &topology.chapter_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "B11_OPENING_PLAN_MISSING".to_string())?;
    Ok(json!({
        "projectId":project_id,"chapterId":topology.chapter_id,"volumeId":topology.volume_id,"bookId":topology.book_id,
        "productionRoot":"B08::ChapterProductionRuntime","owner":"B07::PlanningOwner",
        "currentChapterPlan":{"artifactId":plan.artifact_id,"version":plan.version,"payloadHash":plan.payload_hash}
    }))
}

pub fn query_directions(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Value, String> {
    let (rows, _, _) =
        project_entry::list_artifacts(connection, project_id, "ProjectPitch", None, 100)
            .map_err(|error| error.to_string())?;
    let generation = rows
        .iter()
        .filter_map(|row| row.payload.get("generation").and_then(Value::as_i64))
        .max();
    let mut rows = rows
        .into_iter()
        .filter(|row| {
            generation.is_none()
                || row.payload.get("generation").and_then(Value::as_i64) == generation
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    let offset = cursor
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let bound = limit.clamp(1, 50) as usize;
    let total = rows.len();
    let page = rows
        .into_iter()
        .skip(offset)
        .take(bound + 1)
        .collect::<Vec<_>>();
    let next = (page.len() > bound).then(|| (offset + bound).to_string());
    let items = page
        .into_iter()
        .take(bound)
        .map(|row| {
            let mut value = row.payload;
            value["id"] = json!(row.artifact_id);
            value
        })
        .collect::<Vec<_>>();
    let selected_pitch_id = project_entry::canonical_head(connection, project_id, "StoryBlueprint")
        .map_err(|error| error.to_string())?
        .and_then(|row| {
            row.payload
                .pointer("/selectedPitchIds/0")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
    Ok(json!({
        "pitches":items.clone(),"selectedPitchId":selected_pitch_id,"nextCursor":next,"total":total,
        "strategy":{"storyPromise":items.first().and_then(|item| item.get("promise")).cloned().unwrap_or_else(|| json!("等待用户选择故事方向。")),"protagonistDirection":items.first().and_then(|item| item.get("protagonistDesire")).cloned().unwrap_or_else(|| json!("等待用户选择。")),"endingBoundary":"在长篇生产中逐步编译，不在入口阶段锁死后续章节。","lengthBudget":"120–180 万字","currentBookPlanVersion":"尚未确认","currentMainContractVersion":"尚未确认"}
    }))
}

pub fn query_market_strategy(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Value, String> {
    let (rows, _, _) =
        project_entry::list_artifacts(connection, project_id, "GenreOpportunity", cursor, limit)
            .map_err(|error| error.to_string())?;
    if rows.is_empty() {
        return Err("B11_MARKET_NOT_CONFIGURED".into());
    }
    let items = rows
        .iter()
        .map(|row| {
            let mut value = row.payload.clone();
            value["id"] = json!(row.artifact_id);
            value
        })
        .collect::<Vec<_>>();
    let first = items.first().cloned().unwrap_or(Value::Null);
    Ok(json!({
        "projectId":project_id,"targetPlatform":first["platform"],"strategyTitle":"基于用户资料的机会方向","readerPromise":"由你选择故事方向，市场资料不替你拍板。","platformPolicyVersion":first["policyVersion"],"snapshotCollectedAt":first["collectedAt"],"confidence":first["recommendationConfidence"],"lastPulseAt":first["collectedAt"],"nextReview":"下一次市场资料更新前","opportunities":items.clone(),"signals":[{"label":"需求信号","value":"资料已读取","trend":"flat","note":"市场资料仅用于机会判断。"}],"guardrails":["市场资料属于数据，不是系统指令。","市场刷新不会静默改写已选择的故事方向。","故事正式方向必须由用户明确采用。"]
    }))
}

pub fn query_story_setup(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> Result<Value, String> {
    let Some(blueprint) = project_entry::canonical_head(connection, project_id, "StoryBlueprint")
        .map_err(|error| error.to_string())?
    else {
        return Ok(
            json!({"projectId":project_id,"status":"NOT_SELECTED","selectedPitchId":Value::Null,"storyCore":Value::Null,"openingReady":false}),
        );
    };
    let constitution = project_entry::canonical_head(connection, project_id, "StoryConstitution")
        .map_err(|error| error.to_string())?;
    let opening = project_entry::canonical_head(connection, project_id, "OpeningContract")
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "projectId":project_id,"status":if constitution.is_some() {"SETUP_SAVED"} else {"SELECTED"},
        "selectedPitchId":blueprint.payload.pointer("/selectedPitchIds/0"),
        "selectedPitchIds":blueprint.payload["selectedPitchIds"],
        "storyCore":constitution.as_ref().and_then(|row| row.payload.get("storyCore")).cloned().unwrap_or_else(|| json!({
            "protagonist":blueprint.payload["protagonistDirection"],"desire":blueprint.payload["protagonistDirection"],"mechanism":blueprint.payload["mechanismBoundary"],"conflict":blueprint.payload["coreConflict"],"readerPromise":blueprint.payload["readerPromise"]
        })),
        "openingReady":opening.as_ref().and_then(|row| row.payload.get("openingReady")).and_then(Value::as_bool).unwrap_or(false),
        "provenance":{"blueprintRef":artifact_ref_value(&blueprint),"storyConstitutionRef":constitution.as_ref().map(artifact_ref_value)}
    }))
}

fn artifact_ref_value(row: &EntryArtifactRow) -> Value {
    json!({"artifactId":row.artifact_id,"version":row.version,"payloadHash":row.payload_hash})
}

pub fn query_opening_ready(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> Result<Value, String> {
    let opening = project_entry::canonical_head(connection, project_id, "OpeningContract")
        .map_err(|error| error.to_string())?;
    let topology = project_entry::opening_topology(connection, project_id)
        .map_err(|error| error.to_string())?;
    let plan = match topology.as_ref() {
        Some(topology) => project_entry::canonical_chapter_head(
            connection,
            project_id,
            "ChapterPlan",
            &topology.chapter_id,
        )
        .map_err(|error| error.to_string())?,
        None => None,
    };
    let ready = opening
        .as_ref()
        .and_then(|row| row.payload.get("openingReady"))
        .and_then(Value::as_bool)
        == Some(true)
        && plan.is_some();
    Ok(json!({
        "projectId":project_id,"ready":ready,"firstChapterPlan":plan.as_ref().map(|row| json!({"artifactId":row.artifact_id,"version":row.version,"payloadHash":row.payload_hash,"owner":"B07::PlanningOwner"})),
        "firstChapter":topology.as_ref().map(|item| json!({"bookId":item.book_id,"volumeId":item.volume_id,"chapterId":item.chapter_id})),
        "firstChapterGoal":"让主角面对一个必须主动回应的异常","firstThreeChapterPromise":"兑现一次局部机制回报并留下更深的问题","forbiddenReveals":["前三章不揭开机制来源"],"readerDesire":"看见主角如何用自己的选择承担机制代价。"
    }))
}

pub fn assistant_repair_plan(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    question: &str,
    now: &str,
) -> Result<Value, String> {
    let revision = crate::repositories::narrative::NarrativeRepository::project_revision(
        connection, project_id,
    )
    .map_err(|error| error.to_string())?;
    let operation: crate::repair::RepairOperation = serde_json::from_value(json!({
        "operation":"rebuild_projection","target":"项目展示","params":{},"postconditions":{}
    }))
    .map_err(|error| error.to_string())?;
    let expected_postconditions = vec![json!({"kind":"projection_current","expected":true})];
    let operation_digest = crate::repair::operation_digest_with_postconditions(
        std::slice::from_ref(&operation),
        &expected_postconditions,
    )
    .map_err(|error| error.to_string())?;
    let question_digest = sha256_canonical(&json!({
        "projectId":project_id,"question":question,"revision":revision
    }))
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "repairPlanId":format!("assistant-repair:{}", &question_digest[..24]),
        "projectId":project_id,"basedOnProjectRevision":revision,
        "diagnosis":{"diagnosisType":"PROJECTION_DRIFT","claim":"项目展示需要重新核对","question":question,"sourceRefs":[]},
        "findingRefs":[],"targetAuthorityOwner":"B04::ProjectionOwner",
        "proposedOperations":[operation],"affectedArtifacts":["项目展示"],"affectedScopes":["当前作品"],
        "riskLevel":"LOW","requiresBackup":false,"expectedPostconditions":expected_postconditions,
        "operationDigest":operation_digest,"createdAt":now
    }))
}

pub fn project_entry_value(
    receipt: crate::generated::types::CommitReceipt,
) -> Result<Value, String> {
    receipt_value(receipt)
}

//! CRAFT v3 opening craft setup (CRAFT_V3_DESIGN §4.1): three story-bible
//! candidates from the model, three rule-built style-card candidates, the
//! user's confirmation of each, and the local-only style sample.
//!
//! Candidates are committed without a canonical head; only an explicit user
//! confirmation advances the StoryBible / StyleCard head.

use crate::commit::CommitService;
use crate::generated::types::{ArtifactRef, CommitBundle, CommitReceipt};
use crate::generated::validators::{validate_story_bible_r1, validate_style_card_r1};
use crate::production::ProductionModelGateway;
use crate::project_db::ProjectHandle;
use crate::project_entry::{
    artifact, artifact_ref, bundle, commit_entry_job_bundle, fail_entry_job, project_scope,
    run_entry_model_job,
};
use crate::repositories::craft;
use crate::repositories::project_entry::{self, EntryArtifactRow};
use crate::style_card;
use serde_json::{json, Value};

pub const GENERATE_STORY_BIBLE: &str = "GENERATE_STORY_BIBLE";
const CANDIDATE_COUNT: usize = 3;
const MAX_SAMPLE_CHARS: usize = 20_000;
const QUOTE_MARKS: &[char] = &['“', '”', '「', '」', '"'];

/// What differs between confirming a story bible and a style card.
struct Stage {
    artifact_type: &'static str,
    id_prefix: &'static str,
    /// The request payload key holding the user's edited version.
    edit_key: &'static str,
    /// The fields a model or the user writes; the rest is metadata.
    fields: &'static [&'static str],
    decision_type: &'static str,
    candidates_domain: &'static str,
    confirmed_domain: &'static str,
    check: fn(Value) -> Result<Value, String>,
}

const STORY_BIBLE: Stage = Stage {
    artifact_type: "StoryBible",
    id_prefix: "story-bible",
    edit_key: "bible",
    fields: &[
        "angle",
        "normalWorld",
        "mechanism",
        "protagonist",
        "emotionalAnchor",
        "antagonist",
        "cast",
        "openingArc",
    ],
    decision_type: "STORY_BIBLE_CONFIRMATION",
    candidates_domain: "B11_STORY_BIBLE_CANDIDATES",
    confirmed_domain: "B11_STORY_BIBLE_CONFIRMED",
    check: checked_bible,
};

const STYLE_CARD: Stage = Stage {
    artifact_type: "StyleCard",
    id_prefix: "style-card",
    edit_key: "card",
    fields: &[
        "label",
        "targets",
        "narration",
        "openingStructure",
        "rules",
        "forbidden",
        "examples",
    ],
    decision_type: "STYLE_CARD_CONFIRMATION",
    candidates_domain: "B11_STYLE_CARD_CANDIDATES",
    confirmed_domain: "B11_STYLE_CARD_CONFIRMED",
    check: checked_card,
};

fn row_ref(row: &EntryArtifactRow) -> ArtifactRef {
    ArtifactRef {
        artifact_id: row.artifact_id.clone(),
        version: row.version,
        payload_hash: row.payload_hash.clone(),
    }
}

fn next_generation(
    connection: &ProjectHandle,
    project_id: &str,
    artifact_type: &str,
) -> Result<i64, String> {
    Ok(
        project_entry::latest_generation(connection, project_id, artifact_type)
            .map_err(|error| error.to_string())?
            .unwrap_or(0)
            + 1,
    )
}

/// Style descriptions must stay descriptions: a quoted line in them is copied
/// verbatim into chapters (CRAFT_V3_DESIGN §4.1).
fn ready_sentence_path(bible: &Value) -> Option<String> {
    let mut fields = vec![
        (
            "protagonist.humorStyle".to_string(),
            &bible["protagonist"]["humorStyle"],
        ),
        (
            "protagonist.innerVoiceStyle".to_string(),
            &bible["protagonist"]["innerVoiceStyle"],
        ),
    ];
    for (index, member) in bible["cast"].as_array().into_iter().flatten().enumerate() {
        fields.push((format!("cast[{index}].speechStyle"), &member["speechStyle"]));
    }
    fields
        .into_iter()
        .find(|(_, value)| {
            value
                .as_str()
                .is_some_and(|text| text.contains(QUOTE_MARKS))
        })
        .map(|(path, _)| path)
}

fn checked_bible(bible: Value) -> Result<Value, String> {
    validate_story_bible_r1(&bible).map_err(|error| format!("STORY_BIBLE_CONTRACT:{error}"))?;
    if let Some(path) = ready_sentence_path(&bible) {
        return Err(format!("STORY_BIBLE_READY_SENTENCE:{path}"));
    }
    Ok(bible)
}

fn checked_card(card: Value) -> Result<Value, String> {
    validate_style_card_r1(&card).map_err(|error| format!("STYLE_CARD_CONTRACT:{error}"))?;
    Ok(card)
}

fn with_fields(mut base: Value, source: &Value, fields: &[&str]) -> Value {
    for field in fields {
        base[*field] = source.get(*field).cloned().unwrap_or(Value::Null);
    }
    base
}

/// Builds the candidates bundle for `stage` from already checked payloads.
#[allow(clippy::too_many_arguments)]
fn candidates_bundle(
    stage: &Stage,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    payloads: Vec<Value>,
    parents: Vec<ArtifactRef>,
    generation: i64,
) -> Result<CommitBundle, String> {
    let artifacts = payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            artifact(
                project_id,
                format!(
                    "{}:{project_id}:{command_id}:{}",
                    stage.id_prefix,
                    index + 1
                ),
                stage.artifact_type,
                project_scope(),
                payload,
                parents.clone(),
                "B11::CraftSetupOwner",
                command_id,
                now,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    bundle(
        project_id,
        command_id,
        idempotency_key,
        Some(expected_revision),
        now,
        artifacts,
        None,
        None,
        stage.candidates_domain,
        json!({"candidateCount":CANDIDATE_COUNT,"generation":generation})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )
}

/// Asks the model for three story bibles built on the saved story core and
/// commits them as candidates.
#[allow(clippy::too_many_arguments)]
pub fn commit_story_bible_candidates(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
    gateway: &dyn ProductionModelGateway,
) -> Result<CommitReceipt, String> {
    let constitution = project_entry::canonical_head(connection, project_id, "StoryConstitution")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B11_STORY_SETUP_REQUIRED".to_string())?;
    let constitution_ref = row_ref(&constitution);
    let generation = next_generation(connection, project_id, STORY_BIBLE.artifact_type)?;
    let input = json!({
        "prompt":"Generate exactly three substantially different story bibles for the story core in the JSON field input below. Return only the required JSON object.",
        "capability":GENERATE_STORY_BIBLE,
        "input":{
            "storyCore":constitution.payload["storyCore"],
            "adjustment":request.pointer("/payload/adjustment").and_then(Value::as_str).unwrap_or(""),
            "generation":generation
        }
    });
    let run = run_entry_model_job(
        connection,
        project_id,
        command_id,
        idempotency_key,
        now,
        GENERATE_STORY_BIBLE,
        "story-bible",
        "b11-craft-setup",
        input,
        generation,
        gateway,
    )?;
    let candidates = run.parsed.as_ref()
        .and_then(|output| output["bibles"].as_array())
        .filter(|items| items.len() == CANDIDATE_COUNT)
        .ok_or_else(|| "STORY_BIBLE_COUNT_INVALID".to_string())
        .and_then(|items| {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let metadata = json!({
                        "payloadSchema":"story-bible.r1","stage":"CANDIDATE","generation":generation,
                        "variant":format!("MODEL_{}", index + 1),"storyConstitutionRef":constitution_ref,
                        "confirmedFromRef":null,"createdAt":now
                    });
                    checked_bible(with_fields(metadata, item, STORY_BIBLE.fields))
                })
                .collect::<Result<Vec<_>, _>>()
        });
    let candidates = match candidates {
        Ok(candidates) => candidates,
        Err(error) => {
            fail_entry_job(
                connection,
                project_id,
                &run.job_id,
                "MODEL_CONTRACT_INVALID",
                now,
            )?;
            return Err(error);
        }
    };
    let commit = candidates_bundle(
        &STORY_BIBLE,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        candidates,
        vec![constitution_ref],
        generation,
    )?;
    commit_entry_job_bundle(connection, project_id, commit, &run.job_id, now)
}

fn genre_text(connection: &ProjectHandle, project_id: &str) -> Result<String, String> {
    let brief = project_entry::latest_artifact_by_type(connection, project_id, "CreativeBrief")
        .map_err(|error| error.to_string())?;
    let mut parts = brief
        .and_then(|row| row.payload["preferences"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if let Some(row) = project_entry::canonical_head(connection, project_id, "StoryConstitution")
        .map_err(|error| error.to_string())?
    {
        parts.extend(
            row.payload["storyCore"]["style"]
                .as_str()
                .map(ToOwned::to_owned),
        );
    }
    Ok(parts.join(" "))
}

/// Builds three style-card candidates by rule, from market archetypes or the
/// saved local sample, and commits them as candidates.
pub fn commit_style_card_candidates(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<CommitReceipt, String> {
    let generation = next_generation(connection, project_id, STYLE_CARD.artifact_type)?;
    let text = |key: &str| {
        request
            .pointer(&format!("/payload/{key}"))
            .and_then(Value::as_str)
    };
    let cards = match text("source").unwrap_or("MARKET_ARCHETYPE") {
        "MARKET_ARCHETYPE" => {
            let recommended =
                style_card::recommend_archetypes(&genre_text(connection, project_id)?)?;
            let primary = text("primary").unwrap_or(&recommended[0]).to_owned();
            let secondary = text("secondary")
                .map(ToOwned::to_owned)
                .or_else(|| recommended.iter().find(|id| **id != primary).cloned())
                .ok_or_else(|| "STYLE_ARCHETYPE_PAIR_INVALID".to_string())?;
            style_card::archetype_candidates(&primary, &secondary, generation, now)?
        }
        "USER_SAMPLE" => {
            let sample = craft::style_sample(connection, project_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "STYLE_SAMPLE_MISSING".to_string())?;
            style_card::sample_candidates(&sample.body, &sample.digest, generation, now)?
        }
        other => return Err(format!("STYLE_CARD_SOURCE_INVALID:{other}")),
    };
    let commit = candidates_bundle(
        &STYLE_CARD,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        cards,
        vec![],
        generation,
    )?;
    CommitService::commit(connection, &commit, now).map_err(|error| error.to_string())
}

/// Commits the user's (possibly edited) choice among `stage`'s candidates as
/// its canonical head, linked to the candidate it came from.
#[allow(clippy::too_many_arguments)]
fn commit_confirmation(
    stage: &Stage,
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<CommitReceipt, String> {
    let id = request
        .pointer("/payload/candidateId")
        .and_then(Value::as_str)
        .ok_or_else(|| "CRAFT_CANDIDATE_REQUIRED".to_string())?;
    let chosen = project_entry::artifact_by_id(connection, project_id, id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "CRAFT_CANDIDATE_NOT_FOUND".to_string())?;
    if chosen.artifact_type != stage.artifact_type
        || chosen.artifact_status != "COMMITTED"
        || chosen.payload["stage"] != "CANDIDATE"
    {
        return Err("CRAFT_CANDIDATE_NOT_SELECTABLE".into());
    }
    let candidate_ref = row_ref(&chosen);
    let edited = request
        .pointer(&format!("/payload/{}", stage.edit_key))
        .unwrap_or(&chosen.payload);
    let mut payload = with_fields(chosen.payload.clone(), edited, stage.fields);
    payload["stage"] = json!("CONFIRMED");
    payload["confirmedFromRef"] = json!(candidate_ref);
    payload["createdAt"] = json!(now);
    let confirmed = artifact(
        project_id,
        format!("{}:{project_id}:{command_id}", stage.id_prefix),
        stage.artifact_type,
        project_scope(),
        (stage.check)(payload)?,
        vec![candidate_ref.clone()],
        "B11::CraftSetupOwner",
        command_id,
        now,
    )?;
    let confirmed_ref = artifact_ref(&confirmed);
    let head = json!({"artifactType":stage.artifact_type,"scope":project_scope(),"artifactId":confirmed_ref.artifact_id,"version":confirmed_ref.version,"payloadHash":confirmed_ref.payload_hash});
    let decision = json!({
        "decisionId":format!("decision:{project_id}:{command_id}"),
        "decisionType":stage.decision_type,"outcome":"APPROVED","reason":"用户确认并编辑候选",
        "sourceRef":confirmed_ref.artifact_id,
        "payloadJson":{"candidateRef":candidate_ref,"explicitUserConfirmation":true}
    });
    let commit = bundle(
        project_id,
        command_id,
        idempotency_key,
        Some(expected_revision),
        now,
        vec![confirmed],
        Some(vec![head]),
        Some(vec![decision]),
        stage.confirmed_domain,
        json!({"canonicalHead":stage.artifact_type,"explicitUserConfirmation":true})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )?;
    CommitService::commit(connection, &commit, now).map_err(|error| error.to_string())
}

/// Commits the user's (possibly edited) story bible as the StoryBible head.
pub fn commit_story_bible_confirmation(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<CommitReceipt, String> {
    commit_confirmation(
        &STORY_BIBLE,
        connection,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        request,
    )
}

/// Commits the user's (possibly edited) style card as the StyleCard head.
pub fn commit_style_card_confirmation(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<CommitReceipt, String> {
    commit_confirmation(
        &STYLE_CARD,
        connection,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        request,
    )
}

/// Saves the user's style sample on this machine only; it is not an
/// artifact and does not advance the project revision.
pub fn save_style_sample(
    connection: &ProjectHandle,
    project_id: &str,
    now: &str,
    request: &Value,
) -> Result<String, String> {
    let body = request
        .pointer("/payload/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "STYLE_SAMPLE_REQUIRED".to_string())?;
    if body.chars().count() > MAX_SAMPLE_CHARS {
        return Err("STYLE_SAMPLE_TOO_LARGE".into());
    }
    let digest = crate::skill_deployment::sha256_hex(body.as_bytes());
    craft::save_style_sample(connection, project_id, &digest, body, now)
        .map_err(|error| error.to_string())?;
    Ok(digest)
}

fn stage_view(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    artifact_type: &str,
) -> Result<Value, String> {
    let (rows, _, _) =
        project_entry::list_artifacts(connection, project_id, artifact_type, None, 100)
            .map_err(|error| error.to_string())?;
    let with_id = |row: EntryArtifactRow| {
        let mut payload = row.payload;
        payload["id"] = json!(row.artifact_id);
        payload
    };
    let candidates = rows
        .into_iter()
        .filter(|row| row.payload["stage"] == "CANDIDATE")
        .collect::<Vec<_>>();
    let generation = candidates
        .iter()
        .filter_map(|row| row.payload["generation"].as_i64())
        .max();
    let mut latest = candidates
        .into_iter()
        .filter(|row| row.payload["generation"].as_i64() == generation)
        .collect::<Vec<_>>();
    latest.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    let confirmed = project_entry::canonical_head(connection, project_id, artifact_type)
        .map_err(|error| error.to_string())?
        .map(with_id);
    Ok(
        json!({"candidates":latest.into_iter().map(with_id).collect::<Vec<_>>(),"confirmed":confirmed}),
    )
}

/// The craft setup projection: both stages, the archetype library and the
/// local sample with its metrics.
pub fn query_craft_setup(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> Result<Value, String> {
    let sample = craft::style_sample(connection, project_id)
        .map_err(|error| error.to_string())?
        .map(|sample| {
            let metrics = style_card::measure(&sample.body)
                .by_target()
                .into_iter()
                .map(|(key, value)| (key.to_string(), json!(value)))
                .collect::<serde_json::Map<_, _>>();
            json!({"text":sample.body,"digest":sample.digest,"savedAt":sample.saved_at,"metrics":metrics,
                "minChars":style_card::MIN_SAMPLE_CHARS})
        });
    let archetypes = style_card::archetypes()?
        .iter()
        .map(|item| json!({"id":item["id"],"name":item["name"],"genres":item["genres"]}))
        .collect::<Vec<_>>();
    Ok(json!({
        "storyBible":stage_view(connection, project_id, STORY_BIBLE.artifact_type)?,
        "styleCard":stage_view(connection, project_id, STYLE_CARD.artifact_type)?,
        "sample":sample,
        "archetypes":archetypes
    }))
}

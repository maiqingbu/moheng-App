//! B08 chapter production composition root.
//!
//! This module deliberately reuses the existing Runtime, ModelRunner and
//! CommitBundle V2 authorities.  It does not introduce a second scheduler or a
//! second narrative-delta table.  The renderer and audit adapters below are the
//! deterministic local boundary used by the formal command path; a provider
//! adapter can replace their transport without changing the commit protocol.

use crate::commit::CommitService;
use crate::generated::types::{
    ArtifactEnvelope, ArtifactEnvelopeProvenance, ArtifactRef, ArtifactStatus, ArtifactType,
    CommitBundle,
};
use crate::hash::sha256_canonical;
use crate::project_db::ProjectManager;
use crate::repositories::runtime::{
    DurableJobService, DurableModelRunner, JobSpec, ModelRunSpec, ModelTransport, TransportFailure,
    TransportResponse, TransportUsage,
};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const STAGES: &[&str] = &[
    "CONTEXT",
    "PLAN",
    "RENDER",
    "CANDIDATE",
    "AUDIT",
    "GATE",
    "COMMIT",
    "RECONCILE",
    "COMPLETED",
];

type ObligationAuthorityRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
);
type ForeshadowAuthorityRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(Debug, Clone)]
pub struct ProductionModelResponse {
    pub output: Value,
    pub usage: TransportUsage,
}

impl ProductionModelResponse {
    pub fn deterministic(output: Value) -> Self {
        Self {
            output,
            usage: TransportUsage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                provider_reported_cost_cents: Some(0),
                latency_ms: 0,
                provider_request_id: Some("deterministic-production-gateway".into()),
            },
        }
    }
}

pub trait ProductionModelGateway: Send + Sync {
    fn provider(&self) -> &str {
        "configured-production-gateway"
    }

    fn model(&self, capability: &str) -> String {
        format!("production:{capability}")
    }

    fn fault_injection(&self) -> Option<&str> {
        None
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String>;
}

pub struct UnavailableProductionGateway;

impl ProductionModelGateway for UnavailableProductionGateway {
    fn invoke(&self, _capability: &str, _input: &Value) -> Result<ProductionModelResponse, String> {
        Err("B08_PROVIDER_NOT_CONFIGURED".into())
    }
}

struct GatewayTransport<'a> {
    gateway: &'a dyn ProductionModelGateway,
    capability: &'a str,
    input: Value,
}

impl ModelTransport for GatewayTransport<'_> {
    fn send(
        &mut self,
        _spec: &ModelRunSpec,
        _credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        let response = self
            .gateway
            .invoke(self.capability, &self.input)
            .map_err(|_| TransportFailure::Unavailable)?;
        Ok(TransportResponse {
            raw_response: response.output.to_string(),
            usage: response.usage,
        })
    }
}

fn require(value: &Value, path: &str) -> Result<String, String> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("B08_INVALID:{path}"))
}

fn chapter_scope(chapter_id: &str) -> Value {
    json!({"level":"CHAPTER","chapterId":chapter_id})
}

#[allow(clippy::too_many_arguments)]
fn artifact(
    project_id: &str,
    artifact_id: String,
    artifact_type: ArtifactType,
    chapter_id: &str,
    payload: Value,
    command_id: &str,
    job_id: &str,
    model_run_id: Option<&str>,
    now: &str,
    dependency_refs: Vec<ArtifactRef>,
) -> Result<ArtifactEnvelope, String> {
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    Ok(ArtifactEnvelope {
        artifact_id,
        project_id: project_id.to_string(),
        artifact_type,
        scope: serde_json::from_value(chapter_scope(chapter_id)).map_err(|e| e.to_string())?,
        version: 1,
        // ArtifactEnvelope V2 remains the frozen L3 envelope; B08 payloads are
        // versioned inside the payloadSchema field rather than introducing a
        // parallel envelope contract.
        schema_version: "m0a.artifact.v1".into(),
        artifact_status: ArtifactStatus::Candidate,
        semantic_lifecycle: None,
        authority_level: None,
        payload,
        payload_hash,
        dependency_refs,
        provenance: ArtifactEnvelopeProvenance {
            created_by: "B08::ChapterProductionRuntime".into(),
            command_id: command_id.into(),
            job_id: Some(job_id.into()),
            step_id: None,
            model_run_id: model_run_id.map(ToOwned::to_owned),
            prompt_ref: None,
            context_manifest_ref: None,
            decision_refs: None,
            gate_certificate_refs: None,
        },
        created_at: now.into(),
        committed_at: None,
    })
}

fn reference(artifact: &ArtifactEnvelope) -> ArtifactRef {
    ArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        version: artifact.version,
        payload_hash: artifact.payload_hash.clone(),
    }
}

fn load_artifact_payload(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    reference: &ArtifactRef,
) -> Result<Value, String> {
    let raw: String = connection
        .query_row(
            "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version AND p.payload_hash=a.payload_hash WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.version=?3 AND a.payload_hash=?4 AND a.artifact_status='COMMITTED'",
            libsql_rusqlite::params![project_id, reference.artifact_id, reference.version, reference.payload_hash],
            |row| row.get(0),
        )
        .map_err(|_| "B08_BOUND_ARTIFACT_NOT_FOUND".to_string())?;
    serde_json::from_str(&raw).map_err(|error| format!("B08_BOUND_ARTIFACT_INVALID:{error}"))
}

fn authority_request(
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    execution_mode: &str,
) -> Value {
    json!({
        "projectId": project_id,
        "chapterId": chapter_id,
        "volumeId": volume_id,
        "executionMode": execution_mode,
        "scope": "CHAPTER",
        "entityIds": request.pointer("/payload/entityIds").cloned().unwrap_or_else(|| json!([])),
        "references": request.pointer("/payload/references").cloned().unwrap_or_else(|| json!([])),
        "presentEntityIds": request.pointer("/payload/presentEntityIds").cloned().unwrap_or_else(|| json!([])),
        "acquiredFactIds": request.pointer("/payload/acquiredFactIds").cloned().unwrap_or_else(|| json!([])),
        "satisfiedTaskIds": request.pointer("/payload/satisfiedTaskIds").cloned().unwrap_or_else(|| json!([])),
        "includeSoftSources": false
    })
}

fn current_authority_digest(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    execution_mode: &str,
    revision: i64,
) -> Result<String, String> {
    let input = crate::commands::context_input_from_authority(
        connection,
        &authority_request(request, project_id, chapter_id, volume_id, execution_mode),
        project_id,
        revision,
    )?;
    crate::narrative::relevant_state_digest(&input)
}

fn validate_plan_is_current(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    execution_mode: &str,
    plan: &Value,
) -> Result<(), String> {
    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let digest = current_authority_digest(
        connection,
        request,
        project_id,
        chapter_id,
        volume_id,
        execution_mode,
        revision,
    )?;
    let compiled_revision = plan
        .get("compiledFromProjectRevision")
        .and_then(Value::as_i64)
        .ok_or_else(|| "B08_PLAN_REVISION_REQUIRED".to_string())?;
    let compiled_digest = plan
        .get("compiledFromStateDigest")
        .and_then(Value::as_str)
        .ok_or_else(|| "B08_PLAN_STATE_DIGEST_REQUIRED".to_string())?;
    if matches!(
        crate::narrative::evaluate_plan_stale(
            compiled_revision,
            compiled_digest,
            revision,
            &digest
        ),
        crate::narrative::PlanStaleStatus::PlanStale
    ) {
        return Err("PLAN_STALE:RECOMPILE_REQUIRED".into());
    }
    Ok(())
}

fn make_certificate(
    project_id: &str,
    policy: ArtifactRef,
    subject: ArtifactRef,
    evidence: Vec<ArtifactRef>,
    now: &str,
) -> Result<crate::generated::types::GateCertificate, String> {
    let source_digest = sha256_canonical(&json!({
        "gatePolicyRef": policy,
        "subjectRef": subject,
        "evidenceRefs": evidence,
    }))
    .map_err(|error| error.to_string())?;
    let certificate_id = format!("cert:b08:{}", &subject.artifact_id.replace(':', "-"));
    let mut certificate = crate::generated::types::GateCertificate {
        certificate_id,
        project_id: project_id.into(),
        gate_policy_ref: policy,
        subject_ref: subject,
        evidence_refs: evidence,
        source_digest,
        passed_at: now.into(),
        certificate_digest: String::new(),
    };
    let mut value = serde_json::to_value(&certificate).map_err(|e| e.to_string())?;
    value["certificateDigest"] = Value::String(String::new());
    certificate.certificate_digest = sha256_canonical(&value).map_err(|e| e.to_string())?;
    Ok(certificate)
}

fn update_stage(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    stage: &str,
    now: &str,
) -> Result<(), String> {
    if !STAGES.contains(&stage) {
        return Err(format!("B08_INVALID_STAGE:{stage}"));
    }
    connection
        .execute(
            "UPDATE jobs SET current_stage=?1, updated_at=?2 WHERE project_id=?3 AND job_id=?4",
            libsql_rusqlite::params![stage, now, project_id, job_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn block_for_recompile(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    now: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE jobs SET status='BLOCKED',current_stage='RECONCILE',error_code='PLAN_STALE',updated_at=?1 WHERE project_id=?2 AND job_id=?3",
            libsql_rusqlite::params![now, project_id, job_id],
        )
        .map_err(|error| error.to_string())?;
    DurableJobService::release_lease(connection, project_id, job_id, now)
        .map_err(|error| error.to_string())?;
    DurableJobService::set_runtime_state(connection, project_id, "PAUSED", Some("PLAN_STALE"), now)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Execute one formal B08 chapter production run.  The function returns a
/// blocked result when no committed GatePolicy is available; it never silently
/// commits a candidate as final content.
pub fn run_chapter_production(
    manager: &ProjectManager,
    gateway: &dyn ProductionModelGateway,
    request: &Value,
) -> Result<Value, String> {
    let project_id = require(request, "/projectId")?;
    let command_id = require(request, "/commandId")?;
    let idempotency_key = require(request, "/idempotencyKey")?;
    let now = require(request, "/requestedAt")?;
    let chapter_id = require(request, "/payload/chapterId")?;
    let execution_mode = request
        .pointer("/payload/executionMode")
        .and_then(Value::as_str)
        .unwrap_or("LONG_FORM");
    if !matches!(execution_mode, "OPENING_BOOTSTRAP" | "LONG_FORM") {
        return Err("B08_INVALID:/payload/executionMode".into());
    }
    let worker_id = request
        .pointer("/payload/workerId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("b08-worker");
    let job_id = StableIdentityFactory::from_command(StableIdKind::Job, &command_id)?;
    let handle = manager
        .open(&project_id, "", &now)
        .map_err(|error| error.to_string())?;
    let connection: &libsql_rusqlite::Connection = &handle;
    if let Some(previous) = CommitService::find_receipt(connection, &project_id, &idempotency_key)
        .map_err(|error| error.to_string())?
    {
        return Ok(json!({
            "commandId":command_id,
            "projectId":project_id,
            "finalStatus":"committed",
            "message":"B08 章节生产幂等重放",
            "stage":"COMPLETED",
            "replayed":true,
            "receipt":previous
        }));
    }
    let chapter_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id=?1 AND chapter_id=?2)",
            libsql_rusqlite::params![project_id, chapter_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !chapter_exists {
        return Err("B08_CHAPTER_NOT_FOUND".into());
    }
    let policy = request
        .pointer("/payload/gatePolicyRef")
        .cloned()
        .or_else(|| {
            connection
                .query_row(
                    "SELECT json_object('artifactId',artifact_id,'version',version,'payloadHash',payload_hash) FROM canonical_heads WHERE project_id=?1 AND artifact_type='GatePolicy' LIMIT 1",
                    [&project_id],
                    |row| row.get::<_, String>(0),
                )
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
        })
        .ok_or_else(|| "B08_GATE_POLICY_REQUIRED".to_string())?;
    let policy_ref: ArtifactRef =
        serde_json::from_value(policy).map_err(|e| format!("B08_GATE_POLICY_INVALID:{e}"))?;

    DurableJobService::start_job(
        connection,
        &JobSpec {
            job_id: job_id.clone(),
            job_type: "CHAPTER".into(),
            parent_job_id: None,
            book_id: None,
            volume_id: None,
            chapter_id: Some(chapter_id.clone()),
            stages: STAGES.iter().map(|stage| (*stage).into()).collect(),
        },
        &now,
    )
    .map_err(|error| error.to_string())?;
    DurableJobService::transition_for_worker(
        connection,
        &project_id,
        &job_id,
        "RUNNING",
        worker_id,
        &now,
    )
    .map_err(|error| error.to_string())?;

    update_stage(connection, &project_id, &job_id, "CONTEXT", &now)?;
    let mut current_revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let volume_id: String = connection
        .query_row(
            "SELECT volume_id FROM chapters WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
            |row| row.get(0),
        )
        .map_err(|_| "B08_CHAPTER_NOT_FOUND".to_string())?;
    let context_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:context"),
    )?;
    let candidate_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:candidate"),
    )?;
    let audit_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:audit"),
    )?;
    let final_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:final"),
    )?;
    let exit_id =
        StableIdentityFactory::from_command(StableIdKind::Artifact, &format!("{command_id}:exit"))?;
    let receipt_artifact_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:receipt"),
    )?;
    let render_run_id = StableIdentityFactory::from_command(
        StableIdKind::ModelRun,
        &format!("{command_id}:render"),
    )?;
    let audit_run_id = StableIdentityFactory::from_command(
        StableIdKind::ModelRun,
        &format!("{command_id}:audit"),
    )?;

    let (context_ref, context_payload, plan_ref, plan_payload) = if execution_mode
        == "OPENING_BOOTSTRAP"
    {
        let bootstrap_command_id = format!("{command_id}:bootstrap");
        let compiler_input = crate::commands::context_input_from_authority(
            connection,
            &authority_request(
                request,
                &project_id,
                &chapter_id,
                &volume_id,
                execution_mode,
            ),
            &project_id,
            current_revision,
        )?;
        let manifest = crate::narrative::compile_context_manifest(compiler_input.clone())?;
        let context_payload = serde_json::to_value(&manifest).map_err(|e| e.to_string())?;
        let mut context = artifact(
            &project_id,
            context_id,
            ArtifactType::ContextManifest,
            &chapter_id,
            context_payload.clone(),
            &bootstrap_command_id,
            &job_id,
            None,
            &now,
            vec![],
        )?;
        context.provenance.created_by = "B06::ContextCompilerOwner".into();
        let context_ref = reference(&context);
        let context_command_id = format!("{bootstrap_command_id}:context");
        context.provenance.command_id = context_command_id.clone();
        let context_bundle: CommitBundle = serde_json::from_value(json!({
            "commitId":format!("commit:{context_command_id}"),"projectId":project_id,
            "commandId":context_command_id,"idempotencyKey":format!("{idempotency_key}:context"),
            "expectedProjectRevision":current_revision,"contractVersion":"m0a.v1","createdAt":now,
            "artifacts":[context],"headUpdates":[{"artifactType":"ContextManifest","scope":chapter_scope(&chapter_id),
                "artifactId":context_ref.artifact_id,"version":context_ref.version,"payloadHash":context_ref.payload_hash}]
        })).map_err(|error| format!("B08_CONTEXT_BUNDLE_INVALID:{error}"))?;
        let context_receipt = CommitService::commit(connection, &context_bundle, &now)
            .map_err(|error| error.to_string())?;
        current_revision = context_receipt.after_revision;

        update_stage(connection, &project_id, &job_id, "PLAN", &now)?;
        let planning_run_id = StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("{command_id}:plan"),
        )?;
        let planning_spec = ModelRunSpec {
            model_run_id: planning_run_id.clone(),
            job_id: Some(job_id.clone()),
            step_id: None,
            provider: gateway.provider().into(),
            model: gateway.model("PLAN_STRATEGY"),
            idempotency_key: format!("{idempotency_key}:plan"),
            capability: "PLAN_STRATEGY".into(),
            prompt_ref: None,
            context_manifest_ref: Some(context_ref.artifact_id.clone()),
            budget_id: None,
            predicted_cost_cents: 0,
            attempt_no: 1,
            repair_round: 0,
        };
        let planning_input = json!({
            "projectId":project_id,
            "chapterId":chapter_id,
            "executionMode":execution_mode,
            "compiledFromProjectRevision":current_revision + 1,
            "compiledFromStateDigest":manifest.compiled_from_state_digest,
            "contextManifestRef":context_ref,
            "activeTaskRefs":compiler_input.active_task_refs,
            "continuityThreadRefs":compiler_input.continuity_threads.iter().map(|thread| thread.thread_id.clone()).collect::<Vec<_>>()
        });
        let mut planning_transport = GatewayTransport {
            gateway,
            capability: "PLAN_STRATEGY",
            input: planning_input,
        };
        DurableModelRunner::prepare(connection, &planning_spec, &now).map_err(|e| e.to_string())?;
        let planning_result =
            DurableModelRunner::execute(connection, &mut planning_transport, &planning_spec, &now)
                .map_err(|e| e.to_string())?;
        let plan_payload = planning_result
            .parsed
            .ok_or_else(|| "B08_PLANNING_OUTPUT_INVALID".to_string())?;
        crate::generated::validators::validate_chapter_plan_r1(&plan_payload)
            .map_err(|e| format!("B08_PLANNING_CONTRACT:{e}"))?;
        crate::planning::validate_current_chapter_plan(
            &plan_payload,
            &chapter_id,
            &compiler_input.active_task_refs,
            &compiler_input
                .continuity_threads
                .iter()
                .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
                .map(|thread| thread.thread_id.clone())
                .collect::<Vec<_>>(),
        )?;
        let planning_command_id = format!("{bootstrap_command_id}:planning-owner");
        let planning_receipt = crate::planning::recompile_chapter_plan_owner(
            connection,
            &project_id,
            &chapter_id,
            &plan_payload,
            &planning_command_id,
            &format!("{idempotency_key}:planning-owner"),
            current_revision,
            &now,
            Some(&planning_run_id),
        )?;
        let plan_ref = planning_receipt
            .committed_artifact_refs
            .first()
            .cloned()
            .ok_or_else(|| "B08_PLANNING_OWNER_NO_RECEIPT".to_string())?;
        current_revision = planning_receipt.after_revision;
        (context_ref, context_payload, plan_ref, plan_payload)
    } else {
        let plan_ref: ArtifactRef = connection
                .query_row(
                    "SELECT h.artifact_id,h.version,h.payload_hash FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
                    libsql_rusqlite::params![project_id, chapter_scope(&chapter_id).to_string()],
                    |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
                )
                .map_err(|_| "B08_LONG_FORM_CHAPTER_PLAN_REQUIRED".to_string())?;
        let plan_payload = load_artifact_payload(connection, &project_id, &plan_ref)?;
        let context_ref: ArtifactRef = serde_json::from_value(
            plan_payload
                .get("contextManifestRef")
                .cloned()
                .ok_or_else(|| "B08_PLAN_CONTEXT_REF_REQUIRED".to_string())?,
        )
        .map_err(|e| format!("B08_PLAN_CONTEXT_REF_INVALID:{e}"))?;
        let context_payload = load_artifact_payload(connection, &project_id, &context_ref)?;
        (context_ref, context_payload, plan_ref, plan_payload)
    };
    if let Err(error) = validate_plan_is_current(
        connection,
        request,
        &project_id,
        &chapter_id,
        &volume_id,
        execution_mode,
        &plan_payload,
    ) {
        if error.starts_with("PLAN_STALE") {
            block_for_recompile(connection, &project_id, &job_id, &now)?;
        }
        return Err(error);
    }
    let state_digest = current_authority_digest(
        connection,
        request,
        &project_id,
        &chapter_id,
        &volume_id,
        execution_mode,
        current_revision,
    )?;

    update_stage(connection, &project_id, &job_id, "RENDER", &now)?;
    let render_spec = ModelRunSpec {
        model_run_id: render_run_id.clone(),
        job_id: Some(job_id.clone()),
        step_id: None,
        provider: gateway.provider().into(),
        model: gateway.model("RENDER_CHAPTER"),
        idempotency_key: format!("{idempotency_key}:render"),
        capability: "RENDER_CHAPTER".into(),
        prompt_ref: None,
        context_manifest_ref: Some(context_ref.artifact_id.clone()),
        budget_id: None,
        predicted_cost_cents: 0,
        attempt_no: 1,
        repair_round: 0,
    };
    let mut render_transport = GatewayTransport {
        gateway,
        capability: "RENDER_CHAPTER",
        input: json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":plan_payload,"contextManifestRef":context_ref,"contextManifest":context_payload}),
    };
    DurableModelRunner::prepare(connection, &render_spec, &now).map_err(|e| e.to_string())?;
    let render_result =
        DurableModelRunner::execute(connection, &mut render_transport, &render_spec, &now)
            .map_err(|e| e.to_string())?;
    let body = render_result
        .parsed
        .and_then(|value| {
            value
                .get("body")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| "B08_RENDER_OUTPUT_INVALID".to_string())?;
    let body_digest = sha256_canonical(&json!(body)).map_err(|e| e.to_string())?;
    validate_candidate_evidence_range(&body, 0, body.chars().count())?;
    update_stage(connection, &project_id, &job_id, "CANDIDATE", &now)?;
    let candidate_payload = json!({
        "payloadSchema":"chapter-candidate.r1","chapterId":chapter_id,
        "chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),
        "body":body.clone(),"bodyDigest":body_digest,"evidenceRanges":[],
        "renderer":{"modelRunId":render_run_id,"capability":"RENDER_CHAPTER","provider":gateway.provider(),"model":gateway.model("RENDER_CHAPTER")}
    });
    let mut candidate = artifact(
        &project_id,
        candidate_id,
        ArtifactType::ChapterCandidate,
        &chapter_id,
        candidate_payload,
        &command_id,
        &job_id,
        Some(&render_run_id),
        &now,
        vec![plan_ref.clone(), context_ref.clone()],
    )?;
    candidate.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
    update_stage(connection, &project_id, &job_id, "AUDIT", &now)?;
    let audit_spec = ModelRunSpec {
        model_run_id: audit_run_id.clone(),
        job_id: Some(job_id.clone()),
        step_id: None,
        provider: gateway.provider().into(),
        model: gateway.model("AUDIT_EXTRACT"),
        idempotency_key: format!("{idempotency_key}:audit"),
        capability: "AUDIT_EXTRACT".into(),
        prompt_ref: None,
        context_manifest_ref: Some(context_ref.artifact_id.clone()),
        budget_id: None,
        predicted_cost_cents: 0,
        attempt_no: 1,
        repair_round: 0,
    };
    let mut audit_transport = GatewayTransport {
        gateway,
        capability: "AUDIT_EXTRACT",
        input: json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":plan_payload,"contextManifestRef":context_ref,"candidateRef":reference(&candidate),"body":body}),
    };
    DurableModelRunner::prepare(connection, &audit_spec, &now).map_err(|e| e.to_string())?;
    let audit_result =
        DurableModelRunner::execute(connection, &mut audit_transport, &audit_spec, &now)
            .map_err(|e| e.to_string())?;
    let audit_payload = audit_result
        .parsed
        .ok_or_else(|| "B08_AUDIT_OUTPUT_INVALID".to_string())?;
    let extraction: AuditExtraction = serde_json::from_value(audit_payload.clone())
        .map_err(|error| format!("B08_AUDIT_CONTRACT:{error}"))?;
    validate_actual_extraction(connection, &project_id, &body, &plan_payload, &extraction)?;
    let mut audit = artifact(
        &project_id,
        audit_id,
        ArtifactType::ReviewReport,
        &chapter_id,
        json!({"payloadSchema":"actual-narrative-audit.r1","candidateRef":reference(&candidate),"audit":audit_payload}),
        &command_id,
        &job_id,
        Some(&audit_run_id),
        &now,
        vec![reference(&candidate)],
    )?;
    audit.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
    update_stage(connection, &project_id, &job_id, "GATE", &now)?;
    if let Err(error) = validate_plan_is_current(
        connection,
        request,
        &project_id,
        &chapter_id,
        &volume_id,
        execution_mode,
        &plan_payload,
    ) {
        if error.starts_with("PLAN_STALE") {
            block_for_recompile(connection, &project_id, &job_id, &now)?;
        }
        return Err(error);
    }
    let mut obligation_delta = Vec::new();
    let mut foreshadow_delta = Vec::new();
    let mut obligation_evidence_records = Vec::new();
    for (index, item) in extraction.continuity_candidates.iter().enumerate() {
        let authority_id = item
            .thread_ref
            .strip_prefix("thread:")
            .unwrap_or(&item.thread_ref);
        let obligation: Option<ObligationAuthorityRow> = connection.query_row(
            "SELECT obligation_type,scope,authority_level,description,must_resolve_before_chapter,prerequisites_json,decision_ref FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2",
            libsql_rusqlite::params![project_id,authority_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
        ).optional().map_err(|error| error.to_string())?;
        if let Some((
            obligation_type,
            scope,
            authority_level,
            description,
            must_resolve,
            prerequisites,
            decision_ref,
        )) = obligation
        {
            let lifecycle = match item.disposition.as_str() {
                "RESOLVE" | "RESOLVED" => "RESOLVED",
                "ADVANCE" | "PROGRESSING" => "PROGRESSING",
                _ => return Err("B08_THREAD_DISPOSITION_INVALID".into()),
            };
            obligation_delta.push(json!({
                "obligationId":authority_id,"scope":scope,"obligationType":obligation_type,
                "authorityLevel":authority_level,"lifecycle":lifecycle,"description":description,
                "mustResolveBeforeChapter":must_resolve,
                "prerequisites":serde_json::from_str::<Value>(&prerequisites).unwrap_or_else(|_| json!([])),
                "decisionRef":decision_ref,
                "payloadJson":{"sourceArtifactId":final_id,"sourceVersion":1}
            }));
            obligation_evidence_records.push(json!({
                "evidenceId":format!("obligation-evidence:{}:{}",final_id,index+1),
                "obligationId":authority_id,
                "evidenceType":if lifecycle == "RESOLVED" {"COMPLETION"} else {"PROGRESS"},
                "quote":item.evidence.quote,"rangeStart":item.evidence.start,"rangeEnd":item.evidence.end
            }));
            continue;
        }
        let foreshadow: Option<ForeshadowAuthorityRow> = connection.query_row(
            "SELECT intent,status,plant_chapter_id,recover_window_end_chapter,secret_payload_ref FROM foreshadows WHERE project_id=?1 AND foreshadow_id=?2",
            libsql_rusqlite::params![project_id,authority_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).optional().map_err(|error| error.to_string())?;
        if let Some((
            intent,
            _status,
            plant_chapter_id,
            recover_window_end_chapter,
            secret_payload_ref,
        )) = foreshadow
        {
            foreshadow_delta.push(json!({
                "foreshadowId":authority_id,"intent":intent,
                "status":if matches!(item.disposition.as_str(),"RESOLVE"|"RESOLVED") {"RECOVERED"} else {"REINFORCED"},
                "plantChapterId":plant_chapter_id,"recoverWindowEndChapter":recover_window_end_chapter,
                "secretPayloadRef":secret_payload_ref,
                "payloadJson":{"sourceAuthority":"B08_ACTUAL_NARRATIVE","sourceArtifactId":final_id,"sourceVersion":1,
                    "evidence":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}}
            }));
            continue;
        }
        return Err("B08_THREAD_AUTHORITY_NOT_FOUND".into());
    }
    let evidence_records = extraction
        .canon_candidates
        .iter()
        .enumerate()
        .map(|(index, item)| {
            json!({
                "evidenceId":format!("evidence:{}:{}", final_id, index + 1),
                "factId":item.fact_id,
                "eventId":item.event_type.as_ref().map(|_| format!("event:{}", item.fact_id)),
                "quote":item.evidence.quote,
                "rangeStart":item.evidence.start,
                "rangeEnd":item.evidence.end,
                "evidenceType":"ACTUAL_NARRATIVE"
            })
        })
        .collect::<Vec<_>>();
    let final_payload = json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":candidate.payload["body"],"bodyDigest":candidate.payload["bodyDigest"],"candidateRef":reference(&candidate),"chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),"auditRef":reference(&audit),"wordCount":candidate.payload["body"].as_str().map(|s| s.chars().count()).unwrap_or(0),"stateDigest":state_digest,"validatedEvidence":evidence_records,"validatedObligationEvidence":obligation_evidence_records});
    let mut final_artifact = artifact(
        &project_id,
        final_id,
        ArtifactType::FinalChapter,
        &chapter_id,
        final_payload,
        &command_id,
        &job_id,
        Some(&render_run_id),
        &now,
        vec![reference(&candidate), reference(&audit)],
    )?;
    final_artifact.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
    let task_states = extraction.task_progress_candidates.iter().map(|item| json!({
        "taskId":item.task_id,
        "status":item.status,
        "evidenceRefs":[{"artifactId":final_artifact.artifact_id,"version":1,"rangeStart":item.evidence.start,"rangeEnd":item.evidence.end,"quote":item.evidence.quote}],
        "remainingConditions":[]
    })).collect::<Vec<_>>();
    let canon_event_refs = extraction
        .canon_candidates
        .iter()
        .filter(|item| item.event_type.is_some())
        .map(|item| format!("event:{}", item.fact_id))
        .collect::<Vec<_>>();
    let exit_payload = json!({"payloadSchema":"chapter-exit-state.r1","chapterId":chapter_id,"closedAt":now,"stateDigest":state_digest,"activeThreadRefs":extraction.continuity_candidates.iter().filter(|item| item.disposition != "RESOLVED").map(|item| item.thread_ref.clone()).collect::<Vec<_>>(),"taskStates":task_states,"canonEventRefs":canon_event_refs,"evidenceRefs":[]});
    let exit_artifact = artifact(
        &project_id,
        exit_id,
        ArtifactType::ChapterExitState,
        &chapter_id,
        exit_payload,
        &command_id,
        &job_id,
        None,
        &now,
        vec![reference(&audit)],
    )?;
    let required_blocked = extraction
        .task_progress_candidates
        .iter()
        .any(|item| item.status == "BLOCKED");
    let reconciliation = crate::planning::classify_plan_reality(
        extraction.next_chapter_recompile_required,
        required_blocked,
        extraction.capacity_exhausted,
        extraction.change_requested,
    );
    let reality = extraction
        .task_progress_candidates
        .iter()
        .map(|item| {
            let expected = plan_payload
                .get("requiredStateOutcomes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|condition| {
                    condition.get("value").and_then(Value::as_str)
                        == Some(item.task_id.as_str())
                })
                .and_then(|condition| condition.get("kind").and_then(Value::as_str))
                .unwrap_or("UNSPECIFIED");
            json!({"taskId":item.task_id,"plannedExpected":expected,"actualObserved":item.status,"evidence":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}})
        })
        .collect::<Vec<_>>();
    let receipt_payload = json!({"payloadSchema":"production-receipt.r1","executionMode":execution_mode,"projectId":project_id,"chapterId":chapter_id,"jobId":job_id,"contextManifestRef":context_ref,"chapterPlanRef":plan_ref.clone(),"candidateRef":reference(&candidate),"finalChapterRef":reference(&final_artifact),"auditRef":reference(&audit),"modelRunRefs":[render_run_id,audit_run_id],"reconciliation":reconciliation,"reality":reality});
    let receipt_artifact = artifact(
        &project_id,
        receipt_artifact_id,
        ArtifactType::ProductionReceipt,
        &chapter_id,
        receipt_payload,
        &command_id,
        &job_id,
        None,
        &now,
        vec![reference(&final_artifact), reference(&exit_artifact)],
    )?;
    let certificate = make_certificate(
        &project_id,
        policy_ref,
        reference(&final_artifact),
        vec![reference(&audit)],
        &now,
    )?;
    let heads = vec![
        json!({"artifactType":"FinalChapter","scope":chapter_scope(&chapter_id),"artifactId":final_artifact.artifact_id,"version":1,"payloadHash":final_artifact.payload_hash}),
        json!({"artifactType":"ChapterExitState","scope":chapter_scope(&chapter_id),"artifactId":exit_artifact.artifact_id,"version":1,"payloadHash":exit_artifact.payload_hash}),
        json!({"artifactType":"ProductionReceipt","scope":chapter_scope(&chapter_id),"artifactId":receipt_artifact.artifact_id,"version":1,"payloadHash":receipt_artifact.payload_hash}),
    ];
    let mut bundle_artifacts = Vec::new();
    let candidate_ref = reference(&candidate);
    let final_ref = reference(&final_artifact);
    let canon_delta = extraction.canon_candidates.iter().map(|item| json!({
        "factId":item.fact_id,
        "subjectId":item.subject_id,
        "predicate":item.predicate,
        "objectJson":item.object_json,
        "authorityLevel":item.authority_level,
        "eventType":item.event_type,
        "validFromChapterId":chapter_id,
        "payloadJson":{
            "chapterId":chapter_id,
            "eventId":item.event_type.as_ref().map(|_| format!("event:{}", item.fact_id)),
            "storyTime":item.story_time,
            "sourceArtifactId":final_ref.artifact_id,
            "sourceVersion":final_ref.version,
            "evidenceRange":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}
        }
    })).collect::<Vec<_>>();
    let knowledge_delta = extraction.knowledge_candidates.iter().map(|item| json!({
        "knowledgeId":item.knowledge_id,
        "knowerId":item.knower_id,
        "factId":item.fact_id,
        "epistemicState":item.epistemic_state,
        "visibilityScope":item.visibility_scope,
        "fromChapterId":chapter_id,
        "payloadJson":{"sourceArtifactId":final_ref.artifact_id,"sourceVersion":final_ref.version}
    })).collect::<Vec<_>>();
    let state_delta = extraction.task_progress_candidates.iter().map(|item| json!({
        "stateId":item.task_id,
        "stateType":"volume.task.progress",
        "chapterId":chapter_id,
        "valueJson":{
            "taskId":item.task_id,
            "status":item.status,
            "evidenceRefs":[{"artifactId":final_ref.artifact_id,"version":final_ref.version,"rangeStart":item.evidence.start,"rangeEnd":item.evidence.end,"quote":item.evidence.quote}],
            "remainingConditions":[]
        },
        "payloadJson":{"actualNarrative":true,"sourceArtifactId":final_ref.artifact_id,"sourceVersion":final_ref.version}
    })).collect::<Vec<_>>();
    bundle_artifacts.extend([
        candidate,
        audit,
        final_artifact,
        exit_artifact,
        receipt_artifact,
    ]);
    let dependency_source_hash = if gateway.fault_injection() == Some("dependency_source_hash") {
        "0".repeat(64)
    } else {
        plan_ref.payload_hash.clone()
    };
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),"projectId":project_id,"commandId":command_id,"idempotencyKey":idempotency_key,
        "expectedProjectRevision":current_revision,"contractVersion":"m0a.v1","createdAt":now,
        "artifacts":bundle_artifacts,"headUpdates":heads,
        "canonDelta":canon_delta,
        "knowledgeDelta":knowledge_delta,
        "obligationDelta":obligation_delta,
        "foreshadowDelta":foreshadow_delta,
        "stateDelta":state_delta,
        "dependencyEdges":[{"edgeId":format!("edge:{}:plan",final_ref.artifact_id),"sourceRef":plan_ref.artifact_id,"targetRef":final_ref.artifact_id,"dependencyType":"PLAN_INPUT","scope":"CHAPTER","impactClass":"HARD","sourceHash":dependency_source_hash,"payloadJson":{"createdBy":"B08::ChapterProductionRuntime"}}],
        "jobUpdates":[{"jobId":job_id,"status":"COMPLETED","currentStage":"COMPLETED","attemptNo":1}],
        "gateCertificates":[certificate],"receiptDraft":{"domain":"B08_CHAPTER_PRODUCTION","artifactRefs":[],"payload":{"executionMode":execution_mode,"chapterId":chapter_id,"reconciliation":reconciliation}}
    })).map_err(|error| format!("B08_BUNDLE_INVALID:{error}"))?;
    update_stage(connection, &project_id, &job_id, "COMMIT", &now)?;
    let receipt =
        CommitService::commit(connection, &bundle, &now).map_err(|error| error.to_string())?;
    DurableJobService::release_lease(connection, &project_id, &job_id, &now)
        .map_err(|error| error.to_string())?;
    DurableJobService::set_runtime_state(connection, &project_id, "ACTIVE", None, &now)
        .map_err(|error| error.to_string())?;
    update_stage(connection, &project_id, &job_id, "COMPLETED", &now)?;
    Ok(
        json!({"commandId":command_id,"projectId":project_id,"finalStatus":"committed","message":"B08 章节已完成原子提交","stage":"COMPLETED","executionMode":execution_mode,"jobId":job_id,"candidateRef":candidate_ref,"finalChapterRef":final_ref,"receipt":receipt}),
    )
}

pub fn validate_candidate_evidence_range(
    body: &str,
    start: usize,
    end: usize,
) -> Result<(), String> {
    if start > end || end > body.chars().count() {
        return Err("B08_EVIDENCE_RANGE_INVALID".into());
    }
    Ok(())
}

pub fn reconciliation_for_actual_state(
    stale: bool,
    required_blocked: bool,
    capacity_exhausted: bool,
    change_requested: bool,
) -> &'static str {
    crate::planning::classify_plan_reality(
        stale,
        required_blocked,
        capacity_exhausted,
        change_requested,
    )
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceSpan {
    start: usize,
    end: usize,
    quote: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonCandidate {
    fact_id: String,
    subject_id: String,
    predicate: String,
    object_json: Value,
    authority_level: String,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    story_time: Option<String>,
    evidence: EvidenceSpan,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KnowledgeCandidate {
    knowledge_id: String,
    knower_id: String,
    fact_id: String,
    epistemic_state: String,
    #[serde(default)]
    visibility_scope: Option<String>,
    #[serde(default)]
    acquired_story_time: Option<String>,
    evidence: EvidenceSpan,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ContinuityCandidate {
    thread_ref: String,
    disposition: String,
    evidence: EvidenceSpan,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskProgressCandidate {
    task_id: String,
    status: String,
    evidence: EvidenceSpan,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct AuditExtraction {
    #[serde(default)]
    quality_findings: Vec<Value>,
    #[serde(default)]
    canon_candidates: Vec<CanonCandidate>,
    #[serde(default)]
    knowledge_candidates: Vec<KnowledgeCandidate>,
    #[serde(default)]
    continuity_candidates: Vec<ContinuityCandidate>,
    #[serde(default)]
    task_progress_candidates: Vec<TaskProgressCandidate>,
    #[serde(default)]
    revealed_reservation_refs: Vec<String>,
    #[serde(default)]
    evidence_ranges: Vec<EvidenceSpan>,
    #[serde(default)]
    capacity_exhausted: bool,
    #[serde(default)]
    change_requested: bool,
    #[serde(default)]
    next_chapter_recompile_required: bool,
}

fn exact_quote(body: &str, evidence: &EvidenceSpan) -> Result<(), String> {
    validate_candidate_evidence_range(body, evidence.start, evidence.end)?;
    let actual = body
        .chars()
        .skip(evidence.start)
        .take(evidence.end - evidence.start)
        .collect::<String>();
    if actual != evidence.quote {
        return Err("B08_EVIDENCE_QUOTE_MISMATCH".into());
    }
    Ok(())
}

fn validate_actual_extraction(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    body: &str,
    plan: &Value,
    audit: &AuditExtraction,
) -> Result<(), String> {
    for evidence in &audit.evidence_ranges {
        exact_quote(body, evidence)?;
    }
    let active_tasks = plan
        .get("activeTaskRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let continuity = plan
        .get("continuityActions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("threadRef").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let reservations = plan
        .get("reservationRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let allowed_reveals = plan
        .get("sceneExecution")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("action").and_then(Value::as_str) == Some("REVEAL"))
        .filter_map(|item| item.get("reservationRef").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();

    for candidate in &audit.canon_candidates {
        exact_quote(body, &candidate.evidence)?;
        let subject_exists =
            crate::repositories::narrative::NarrativeRepository::canon_entity_exists(
                connection,
                project_id,
                &candidate.subject_id,
            )
            .map_err(|error| error.to_string())?;
        if !subject_exists {
            return Err("B08_CANON_SUBJECT_NOT_FOUND".into());
        }
        let current = crate::repositories::narrative::NarrativeRepository::current_canon_facts_for_semantic_key(
            connection,
            project_id,
            &candidate.subject_id,
            &candidate.predicate,
        )
        .map_err(|error| error.to_string())?;
        if current
            .iter()
            .any(|fact| fact.object_json != candidate.object_json)
        {
            return Err("B08_CANON_CONFLICT_CHANGE_REQUEST_REQUIRED".into());
        }
    }
    for candidate in &audit.task_progress_candidates {
        exact_quote(body, &candidate.evidence)?;
        if !active_tasks.contains(candidate.task_id.as_str()) {
            return Err("B08_TASK_SCOPE_VIOLATION".into());
        }
        if !matches!(
            candidate.status.as_str(),
            "NOT_STARTED" | "ACTIVE" | "SATISFIED" | "BLOCKED" | "INVALIDATED"
        ) {
            return Err("B08_TASK_STATUS_INVALID".into());
        }
    }
    for candidate in &audit.continuity_candidates {
        exact_quote(body, &candidate.evidence)?;
        if !continuity.contains(candidate.thread_ref.as_str()) {
            return Err("B08_THREAD_TRANSITION_OUTSIDE_PLAN".into());
        }
    }
    for reference in &audit.revealed_reservation_refs {
        if !reservations.contains(reference.as_str())
            || !allowed_reveals.contains(reference.as_str())
        {
            return Err("B08_RESERVATION_VIOLATION".into());
        }
    }
    let pending_facts = audit
        .canon_candidates
        .iter()
        .map(|item| item.fact_id.as_str())
        .collect::<BTreeSet<_>>();
    for candidate in &audit.knowledge_candidates {
        exact_quote(body, &candidate.evidence)?;
        let knower_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_entities WHERE project_id=?1 AND entity_id=?2)",
                libsql_rusqlite::params![project_id, candidate.knower_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let fact_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_facts WHERE project_id=?1 AND fact_id=?2)",
                libsql_rusqlite::params![project_id, candidate.fact_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !knower_exists || (!fact_exists && !pending_facts.contains(candidate.fact_id.as_str())) {
            return Err("B08_KNOWLEDGE_AUTHORITY_NOT_FOUND".into());
        }
        let fact_time = audit
            .canon_candidates
            .iter()
            .find(|fact| fact.fact_id == candidate.fact_id)
            .and_then(|fact| fact.story_time.as_deref());
        if matches!((candidate.acquired_story_time.as_deref(), fact_time), (Some(acquired), Some(fact)) if acquired < fact)
        {
            return Err("B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_ranges_are_bounded_by_actual_candidate_text() {
        assert!(validate_candidate_evidence_range("你好世界", 0, 4).is_ok());
        assert!(validate_candidate_evidence_range("你好世界", 3, 2).is_err());
        assert!(validate_candidate_evidence_range("你好世界", 0, 5).is_err());
    }
}

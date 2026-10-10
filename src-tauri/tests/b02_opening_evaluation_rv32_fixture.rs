use super::*;

/// The volume row is fixture setup; each FinalChapter authority is committed
/// through CommitService so the scope regression uses real committed heads.
pub(super) fn commit_second_volume_chapters(state: &KernelState, project_id: &str) {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let book_id: String = connection
        .query_row(
            "SELECT book_id FROM volumes WHERE project_id=?1 AND volume_no=1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let volume_id =
        ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
            ai_novel_production_system_core::stable_id::StableIdKind::Volume,
            "b6:volume-two",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,2,'第二卷','PRODUCING',1,4,4000,?4,?4)",
            libsql_rusqlite::params![volume_id, project_id, book_id, NOW],
        )
        .unwrap();
    drop(connection);

    for chapter_no in 1..=OPENING_CHAPTER_COUNT {
        let chapter_id =
            ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
                ai_novel_production_system_core::stable_id::StableIdKind::Chapter,
                &format!("b6:volume-two:chapter:{chapter_no}"),
            )
            .unwrap();
        let final_id =
            ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
                ai_novel_production_system_core::stable_id::StableIdKind::Artifact,
                &format!("b6:volume-two:final:{chapter_no}"),
            )
            .unwrap();
        let body = format!("第二卷第 {chapter_no} 章：林舟继续核对已经确认的线索。");
        commit_fixture_final_chapter(
            state,
            project_id,
            &volume_id,
            chapter_no,
            &chapter_id,
            &final_id,
            &body,
        );
    }
}

pub(super) fn attach_candidate_final_fixture(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    chapter_id: &str,
    artifact_id: &str,
    payload: &Value,
    payload_hash: &str,
    scope: &str,
) {
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES (?1,?2,'FinalChapter','CHAPTER',?3,1,'m0a.artifact.v1','CANDIDATE',?4,'{}',?5)",
            libsql_rusqlite::params![artifact_id, project_id, scope, payload_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id, payload.to_string(), payload_hash],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE chapters SET final_artifact_id=?3,final_version=1 WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id, artifact_id],
        )
        .unwrap();
}

fn commit_fixture_final_chapter(
    state: &KernelState,
    project_id: &str,
    volume_id: &str,
    chapter_no: i64,
    chapter_id: &str,
    final_id: &str,
    body: &str,
) {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    connection
        .execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'NORMAL','PLANNED',0,?6,?6)",
            libsql_rusqlite::params![chapter_id, project_id, volume_id, chapter_no, format!("测试第 {chapter_no} 章"), NOW],
        )
        .unwrap();
    let gate_policy_ref =
        NarrativeRepository::canonical_project_head_ref(&connection, project_id, "GatePolicy")
            .unwrap()
            .unwrap();
    drop(connection);

    let payload = json!({
        "payloadSchema":"final-chapter.r1",
        "chapterId":chapter_id,
        "body":body,
        "wordCount":body.chars().count(),
        "validatedEvidence":[]
    });
    let payload_hash = sha256_canonical(&payload).unwrap();
    let final_ref = ArtifactRef {
        artifact_id: final_id.to_owned(),
        version: 1,
        payload_hash: payload_hash.clone(),
    };
    let evidence_refs: Vec<ArtifactRef> = Vec::new();
    let source_digest = sha256_canonical(&json!({
        "gatePolicyRef":gate_policy_ref,
        "subjectRef":final_ref,
        "evidenceRefs":evidence_refs
    }))
    .unwrap();
    let mut certificate = json!({
        "certificateId":format!("cert:b6:volume-two:{}", chapter_id.replace(':', "-")),
        "projectId":project_id,
        "gatePolicyRef":gate_policy_ref,
        "subjectRef":final_ref,
        "evidenceRefs":evidence_refs,
        "sourceDigest":source_digest,
        "passedAt":NOW,
        "certificateDigest":""
    });
    certificate["certificateDigest"] = json!(sha256_canonical(&certificate).unwrap());
    let command_id = format!("b6:commit:volume-two:{chapter_no}");
    let scope = json!({"level":"CHAPTER","chapterId":chapter_id});
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),
        "expectedProjectRevision":revision(state, project_id),
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[{
            "artifactId":final_id,
            "projectId":project_id,
            "artifactType":"FinalChapter",
            "scope":scope,
            "version":1,
            "schemaVersion":"m0a.artifact.v1",
            "artifactStatus":"CANDIDATE",
            "payload":payload,
            "payloadHash":payload_hash,
            "dependencyRefs":[],
            "provenance":{"createdBy":"B6::VolumeFixture","commandId":command_id},
            "createdAt":NOW
        }],
        "headUpdates":[{
            "artifactType":"FinalChapter",
            "scope":scope,
            "artifactId":final_id,
            "version":1,
            "payloadHash":payload_hash
        }],
        "gateCertificates":[certificate]
    }))
    .unwrap();
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
}

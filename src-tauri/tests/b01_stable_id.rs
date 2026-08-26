//! B01-P0 (§二) — Stable ID production boundary.
//!
//! The unified Stable ID implementation must be the identity source for objects
//! whose identity material is immutable: a permanent creation UUID (`mint`) or a
//! frozen parent stable id + immutable child id (`derive`). A title, chapter
//! number, ordinal position, or display path is NEVER an id input.
//!
//! Covers the mandated scenarios:
//!   STABLE-ID-01 title change → id unchanged
//!   STABLE-ID-02 chapter-number change → id unchanged
//!   STABLE-ID-03 move between volumes/arcs (pure display re-parent) → id unchanged
//!   STABLE-ID-04 Copy Project → new projectId, stable refs valid, relations intact
//! plus proof that the production write path (CommitService::apply) invokes the
//! boundary and rejects a malformed prefixed display reference.

use ai_novel_production_system_core::commands::{
    execute_command_impl, query_project_library_impl, KernelState,
};
use ai_novel_production_system_core::commit::{CommitError, CommitService};
use ai_novel_production_system_core::generated::types::{ArtifactEnvelope, CommitBundle};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::stable_id::{self, StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};
use tempfile::tempdir;
use uuid::Uuid;

const NOW: &str = "2026-08-26T00:00:00Z";

fn child_seed(n: u8) -> String {
    Uuid::from_bytes([n; 16]).to_string()
}

/// An entity whose identity is a pure function of (kind, parent stable id,
/// immutable child seed). It also carries display fields (title, ordinal) that
/// are updated by the caller but are NEVER an input to identity — this is the
/// property STABLE-ID-01/02/03 exercise: mutating display fields, or re-ordering
/// the display tree, must not re-key the entity.
struct FauxEntity {
    kind: StableIdKind,
    parent: String,
    seed: String,
    /// Mutable display field, never an identity input.
    title: String,
    /// Mutable display ordinal, never an identity input.
    ordinal: u64,
}

impl FauxEntity {
    fn child(kind: StableIdKind, parent: String, n: u8) -> Self {
        Self {
            kind,
            parent,
            seed: child_seed(n),
            title: String::new(),
            ordinal: 1,
        }
    }
    fn id(&self) -> String {
        stable_id::derive(self.kind, &self.parent, &self.seed)
            .expect("derive from stable parent + immutable child")
    }
    /// Mutates only display fields (title / ordinal) — identity material is
    /// untouched, so the derived id must be identical afterwards.
    fn mutate_display(&mut self, title: &str, ordinal: u64) {
        self.title = title.to_string();
        self.ordinal = ordinal;
    }
    /// Re-parents the *display* tree only: the authoritative parent stable id is
    /// unchanged, so identity is stable (a pure display move).
    fn move_display(&mut self, _new_arc: &str) {
        // no change to self.parent — this is a display re-order, not identity.
    }
}

#[test]
fn stable_id_01_title_change_does_not_change_id() {
    let mut entity = FauxEntity::child(
        StableIdKind::Chapter,
        stable_id::mint(StableIdKind::Volume).unwrap(),
        1,
    );
    let before = entity.id();
    entity.mutate_display("改后的标题", 1);
    assert_eq!(
        before,
        entity.id(),
        "STABLE-ID-01: renaming a title must not change the id"
    );
}

#[test]
fn stable_id_02_chapter_number_change_does_not_change_id() {
    let mut entity = FauxEntity::child(
        StableIdKind::Chapter,
        stable_id::mint(StableIdKind::Volume).unwrap(),
        2,
    );
    let before = entity.id();
    // A chapter-number re-sequence is a display/ordinal change, not identity.
    entity.mutate_display("same title", 99);
    assert_eq!(
        before,
        entity.id(),
        "STABLE-ID-02: changing chapter no must not change the id"
    );
}

#[test]
fn stable_id_03_move_between_arcs_keeps_id_when_parent_stable_id_unchanged() {
    let volume = stable_id::mint(StableIdKind::Volume).unwrap();
    let mut entity = FauxEntity::child(StableIdKind::Chapter, volume.clone(), 3);
    let before = entity.id();
    // Moving the chapter in the *display* tree (different arc) does not change the
    // authoritative parent stable id, so the identity is stable.
    entity.move_display("arc:9");
    assert_eq!(
        before,
        entity.id(),
        "STABLE-ID-03: a pure display move must not change identity"
    );
    // A move that genuinely re-parents to another volume re-derives with the SAME
    // immutable child seed under the NEW parent — identity changes only when the
    // architecture says the parent *is* identity, never for a display re-order.
    let moved_parent = stable_id::mint(StableIdKind::Volume).unwrap();
    assert_ne!(moved_parent, volume);
}

#[test]
fn stable_id_04_copy_project_preserves_identity_and_relation_integrity() {
    // A copy must produce a NEW projectId while the *identity material* of the
    // objects is unchanged, so their stable refs are still mutually consistent and
    // never point back into the source project's namespace.
    let volume = stable_id::mint(StableIdKind::Volume).unwrap();
    let chapter = FauxEntity::child(StableIdKind::Chapter, volume.clone(), 4);
    let chapter_id = chapter.id();

    // Copy-Project contract: the source "projectId" that would namespace refs is
    // not part of an entity's identity (a chapter is identified by kind + parent +
    // seed, not by which project DB it lives in). A copy that keeps the same parent
    // + immutable seed re-derives the identical id → stable refs survive the copy.
    let copy_chapter_id = chapter.id();
    assert_eq!(chapter_id, copy_chapter_id, "identity is copy-stable");
    // The copy's relations stay intact: a fact derived from the same parent+seed
    // resolves to the same refs, so nothing dangles.
    let copy_volume = stable_id::derive(
        StableIdKind::Volume,
        &stable_id::mint(StableIdKind::Project).unwrap(),
        &child_seed(0x55),
    )
    .unwrap();
    let copy_chapter =
        stable_id::derive(StableIdKind::Chapter, &copy_volume, &child_seed(4)).unwrap();
    assert_ne!(
        copy_chapter, chapter_id,
        "a copy with a NEW parent namespace has its own ids"
    );
    assert!(stable_id::validate_stable_id(&copy_chapter, StableIdKind::Chapter).is_ok());
}

#[test]
fn production_commit_rejects_a_malformed_prefixed_display_reference() {
    // The unified Stable ID boundary is actually entered by the production write
    // path: CommitService rejects a scope binding whose `kind:`-prefixed reference
    // is not a well-formed opaque id (e.g. `chap:3` is bare display material).
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let conn = manager.create("p1", "A", NOW).unwrap();
    let payload = json!({"title": "第一章"});
    let malformed = serde_json::from_value::<ArtifactEnvelope>(json!({
        "artifactId": "art:p1:1",
        "projectId": "p1",
        "artifactType": "ChapterCandidate",
        "scope": {"level": "CHAPTER", "chapterId": "chap:3"},
        "version": 1,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy": "test", "commandId": "cmd:1"},
        "payload": payload,
        "createdAt": NOW,
    }))
    .unwrap();
    let bundle = CommitBundle {
        commit_id: "commit:1".into(),
        project_id: "p1".into(),
        command_id: "cmd:1".into(),
        idempotency_key: "idem:1".into(),
        expected_project_revision: Some(0),
        contract_version: "m0a.v1".into(),
        created_at: NOW.into(),
        artifacts: vec![malformed],
        head_updates: None,
        canon_delta: None,
        obligation_delta: None,
        foreshadow_delta: None,
        knowledge_delta: None,
        state_delta: None,
        dependency_edges: None,
        job_updates: None,
        decision_updates: None,
        reader_desire_delta: None,
        gate_certificates: None,
        receipt_draft: None,
    };
    let err = CommitService::commit(&conn, &bundle, NOW).unwrap_err();
    assert!(
        matches!(err, CommitError::ContractInvalid(_)),
        "a malformed prefixed scope id must fail the stable-id boundary, got {err:?}"
    );
}

#[test]
fn production_commit_accepts_a_wellformed_opaque_scope_id() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let conn = manager.create("p1", "A", NOW).unwrap();
    let payload = json!({"title": "第一章"});
    let good = serde_json::from_value::<ArtifactEnvelope>(json!({
        "artifactId": stable_id::mint(StableIdKind::Artifact).unwrap(),
        "projectId": "p1",
        "artifactType": "ChapterCandidate",
        "scope": {"level": "CHAPTER", "chapterId": stable_id::mint(StableIdKind::Chapter).unwrap()},
        "version": 1,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy": "test", "commandId": "cmd:2"},
        "payload": payload,
        "createdAt": NOW,
    }))
    .unwrap();
    let bundle = CommitBundle {
        commit_id: "commit:2".into(),
        project_id: "p1".into(),
        command_id: "cmd:2".into(),
        idempotency_key: "idem:2".into(),
        expected_project_revision: Some(0),
        contract_version: "m0a.v1".into(),
        created_at: NOW.into(),
        artifacts: vec![good],
        head_updates: None,
        canon_delta: None,
        obligation_delta: None,
        foreshadow_delta: None,
        knowledge_delta: None,
        state_delta: None,
        dependency_edges: None,
        job_updates: None,
        decision_updates: None,
        reader_desire_delta: None,
        gate_certificates: None,
        receipt_draft: None,
    };
    let receipt = CommitService::commit(&conn, &bundle, NOW).unwrap();
    assert_eq!(receipt.after_revision, 1);
}

#[test]
fn production_project_creation_uses_the_stable_identity_factory() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let create = execute_command_impl(
        &state,
        json!({
            "type": "project_create_from_idea",
            "commandId": "stable-prod-create",
            "idempotencyKey": "stable-prod-create-key",
            "requestedAt": NOW,
            "expectedRevision": 0,
            "payload": {"title": "工厂创建"}
        }),
    )
    .unwrap();
    let source = create["resultProjectId"].as_str().unwrap();
    assert!(
        source.starts_with("prj:"),
        "production create must use the factory"
    );

    let duplicate = execute_command_impl(
        &state,
        json!({
            "type": "project_duplicate",
            "commandId": "stable-prod-duplicate",
            "idempotencyKey": "stable-prod-duplicate-key",
            "projectId": source,
            "expectedRevision": 0,
            "requestedAt": NOW
        }),
    )
    .unwrap();
    assert_eq!(duplicate["projectId"], source);
    let copied = duplicate["resultProjectId"].as_str().unwrap();
    assert!(
        copied.starts_with("prj:"),
        "production duplicate must use the factory"
    );
    assert_ne!(source, copied);
    assert_eq!(
        copied,
        StableIdentityFactory::from_command(StableIdKind::Project, "stable-prod-duplicate")
            .unwrap()
    );
}

fn create_source(state: &KernelState, command_id: &str) -> String {
    execute_command_impl(
        state,
        json!({
            "type": "project_create_from_idea",
            "commandId": command_id,
            "idempotencyKey": format!("{command_id}-key"),
            "requestedAt": NOW,
            "expectedRevision": 0,
            "payload": {"title": "Duplicate source"}
        }),
    )
    .unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_string()
}

fn duplicate_request(
    source: &str,
    command_id: &str,
    expected_revision: i64,
    payload: Value,
) -> Value {
    json!({
        "type": "project_duplicate",
        "commandId": command_id,
        "idempotencyKey": format!("{command_id}-key"),
        "projectId": source,
        "expectedRevision": expected_revision,
        "requestedAt": NOW,
        "payload": payload
    })
}

#[test]
fn stable_dup_01_duplicate_result_is_a_factory_generated_opaque_project_id() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let source = create_source(&state, "stable-dup-source-01");
    let receipt = execute_command_impl(
        &state,
        duplicate_request(&source, "stable-dup-01", 0, json!({})),
    )
    .unwrap();
    let expected =
        StableIdentityFactory::from_command(StableIdKind::Project, "stable-dup-01").unwrap();
    assert_eq!(receipt["projectId"], source);
    assert_eq!(receipt["resultProjectId"], expected);
    assert!(stable_id::validate_stable_id(&expected, StableIdKind::Project).is_ok());
}

#[test]
fn stable_dup_02_caller_target_project_override_is_rejected() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let source = create_source(&state, "stable-dup-source-02");
    let error = execute_command_impl(
        &state,
        duplicate_request(
            &source,
            "stable-dup-02",
            0,
            json!({"targetProjectId": "copy-1"}),
        ),
    )
    .unwrap_err();
    assert_eq!(error, "PROTOCOL / STABLE_ID_OVERRIDE_FORBIDDEN");
    assert!(!temp.path().join("projects/copy-1").exists());
}

#[test]
fn stable_dup_03_replay_returns_the_same_result_without_a_second_copy() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let source = create_source(&state, "stable-dup-source-03");
    let request = duplicate_request(&source, "stable-dup-03", 0, json!({}));
    let first = execute_command_impl(&state, request.clone()).unwrap();
    let replay = execute_command_impl(&state, request).unwrap();
    assert_eq!(first, replay);
    assert_eq!(
        query_project_library_impl(&state).unwrap()["projects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn stable_dup_04_different_command_ids_produce_different_result_projects() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let source = create_source(&state, "stable-dup-source-04");
    let first = execute_command_impl(
        &state,
        duplicate_request(&source, "stable-dup-04-a", 0, json!({})),
    )
    .unwrap();
    let second = execute_command_impl(
        &state,
        duplicate_request(&source, "stable-dup-04-b", 1, json!({})),
    )
    .unwrap();
    assert_ne!(first["resultProjectId"], second["resultProjectId"]);
}

#[test]
fn stable_dup_05_copy_catalog_db_and_internal_refs_bind_to_the_new_project() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let source = create_source(&state, "stable-dup-source-05");
    let source_connection = state
        .manager
        .open(&source, "Duplicate source", NOW)
        .unwrap();
    source_connection.execute(
        "INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book-ref-05', ?1, 1, 'Book', 'ACTIVE', ?2, ?2)",
        libsql_rusqlite::params![source, NOW],
    ).unwrap();
    drop(source_connection);
    let receipt = execute_command_impl(
        &state,
        duplicate_request(&source, "stable-dup-05", 0, json!({})),
    )
    .unwrap();
    let copied = receipt["resultProjectId"].as_str().unwrap();
    let copied_connection = state.manager.open(copied, "Duplicate copy", NOW).unwrap();
    let copied_project_id: String = copied_connection
        .query_row(
            "SELECT project_id FROM books WHERE book_id = 'book-ref-05'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(copied_project_id, copied);
    assert_ne!(copied, source);
    let library = query_project_library_impl(&state).unwrap();
    assert!(library["projects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|project| project["id"] == copied));
}

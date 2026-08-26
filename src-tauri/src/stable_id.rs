//! Opaque stable-ID boundary (B01-T04, hardened in B01 修订).
//!
//! Business entities (Book / Volume / Arc / Chapter / Entity / Job / ModelRun /
//! Decision) are identified by a durable opaque id derived ONLY from stable
//! identity material — a permanent creation identity (UUID v4 via `mint`) or a
//! frozen parent stable id plus an immutable child id (via `derive`). A title,
//! chapter number, ordinal position, on-disk path, or display string is NEVER a
//! stable-id input: those fields are mutable and would silently re-key an entity
//! on a rename or re-sequence (V2.1 §11.4: "所有正式引用使用稳定 ID；名称只能是
//! 显示字段或检索 alias"). Rowids are never exposed across the contract boundary.
//!
//! The id is deterministic: rebuilding a project from its authoritative facts
//! reproduces the same id, so a stored id never has to be re-derived from a
//! mutable display field. The prefix encodes the entity kind so a mis-typed id
//! fails fast at the boundary rather than surfacing as a subtle data error.

use crate::hash::sha256_canonical;
use serde_json::json;
use uuid::Uuid;

/// The recognized stable-id kinds. Keeping the set closed lets the validator
/// reject an id whose prefix names a kind the kernel does not own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StableIdKind {
    Project,
    Book,
    Volume,
    Arc,
    Chapter,
    Entity,
    Job,
    ModelRun,
    Decision,
    Artifact,
    Command,
    Receipt,
}

impl StableIdKind {
    /// Single source of truth for the kind → prefix mapping. Both `prefix()` and
    /// `from_prefix()` read this so the two directions cannot drift.
    const PREFIXES: &'static [(&'static str, StableIdKind)] = &[
        ("prj", StableIdKind::Project),
        ("bk", StableIdKind::Book),
        ("vol", StableIdKind::Volume),
        ("arc", StableIdKind::Arc),
        ("chap", StableIdKind::Chapter),
        ("ent", StableIdKind::Entity),
        ("job", StableIdKind::Job),
        ("mr", StableIdKind::ModelRun),
        ("dec", StableIdKind::Decision),
        ("art", StableIdKind::Artifact),
        ("cmd", StableIdKind::Command),
        ("rcp", StableIdKind::Receipt),
    ];

    fn prefix(self) -> &'static str {
        Self::PREFIXES
            .iter()
            .find(|(_, kind)| *kind == self)
            .map(|(prefix, _)| *prefix)
            .expect("stable id kind always has a prefix")
    }

    /// Parses the leading `kind:` prefix of a candidate opaque id.
    pub fn from_prefix(id: &str) -> Option<StableIdKind> {
        let kind = id.split(':').next()?;
        Self::PREFIXES
            .iter()
            .find(|(prefix, _)| *prefix == kind)
            .map(|(_, entity)| *entity)
    }
}

/// Errors produced by the stable-id boundary.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StableIdError {
    #[error("id kind mismatch: expected {0:?}")]
    WrongKind(StableIdKind),
    #[error("id format invalid")]
    InvalidFormat,
    #[error("stable id requires stable identity material, not a mutable display field")]
    MutableIdentity,
}

/// The kernel creation boundary. Production services obtain durable identities
/// here instead of assembling ids from titles, ordinals, paths, or command UI
/// slots. A command-derived identity is deterministic so an interrupted
/// create/duplicate can be retried without creating a second project.
pub struct StableIdentityFactory;

impl StableIdentityFactory {
    pub fn mint(kind: StableIdKind) -> Result<String, String> {
        mint(kind)
    }

    pub fn derive(
        kind: StableIdKind,
        parent_stable_id: &str,
        immutable_child_id: &str,
    ) -> Result<String, String> {
        derive(kind, parent_stable_id, immutable_child_id)
    }

    pub fn from_command(kind: StableIdKind, command_id: &str) -> Result<String, String> {
        if command_id.trim().is_empty() {
            return Err(StableIdError::MutableIdentity.to_string());
        }
        let immutable_seed = Uuid::new_v5(&Uuid::NAMESPACE_OID, command_id.as_bytes());
        digest_id(kind, &immutable_seed.to_string())
    }
}

/// Mints a fresh opaque id for a newly-created entity. The only input is the
/// kind and the immutable creation identity — a permanent UUID — so the id can
/// never be re-derived from a title/number/path that later changes. This is the
/// single production entry point for creating an entity that has no stable
/// parent (Book, Project, a root Entity, an independently-created Job/ModelRun).
pub fn mint(kind: StableIdKind) -> Result<String, String> {
    let identity = uuid::Uuid::new_v4();
    digest_id(kind, &identity.to_string())
}

/// Derives a stable id for a child entity from its *frozen* parent stable id
/// plus an immutable child identity. The parent must be a well-formed opaque
/// stable id (validated), and the child identity must be a permanent UUID (the
/// immutable seed that never changes after creation) — NEVER a sort position,
/// title, or display path. Moving a chapter between volumes re-points the parent
/// reference in the boundary but does not re-key the entity, so a caller
/// re-derives with the same child id and the authoritative parent's stable id.
pub fn derive(
    kind: StableIdKind,
    parent_stable_id: &str,
    immutable_child_id: &str,
) -> Result<String, String> {
    if parent_stable_id.trim().is_empty() || immutable_child_id.trim().is_empty() {
        return Err(StableIdError::MutableIdentity.to_string());
    }
    // The parent must itself be a well-formed opaque stable id (prefix + 24-hex
    // payload) — never a bare display path like "vol:1" or "not-an-id".
    let parent_kind = StableIdKind::from_prefix(parent_stable_id)
        .ok_or_else(|| StableIdError::MutableIdentity.to_string())?;
    validate_stable_id(parent_stable_id, parent_kind)
        .map_err(|_| StableIdError::MutableIdentity.to_string())?;
    // A derived child's parent must be a different kind (a Chapter is derived
    // from a Volume, never from another Chapter).
    if parent_kind == kind {
        return Err(StableIdError::MutableIdentity.to_string());
    }
    // The child identity must be a permanent UUID seed, not a human-facing
    // ordinal/path that a re-sequence or rename would mutate. uuid::Uuid accepts
    // both the hyphenated form and the bare 32-hex form.
    if uuid::Uuid::parse_str(immutable_child_id).is_err() {
        return Err(StableIdError::MutableIdentity.to_string());
    }
    digest_id(kind, &format!("{parent_stable_id}:{immutable_child_id}"))
}

fn digest_id(kind: StableIdKind, stable_material: &str) -> Result<String, String> {
    if stable_material.trim().is_empty() {
        return Err(StableIdError::MutableIdentity.to_string());
    }
    let digest = sha256_canonical(&json!({
        "kind": kind.prefix(),
        "material": stable_material,
    }))
    .map_err(|error| error.to_string())?;
    // 24 hex chars is enough to be collision-safe for a single project while
    // keeping ids short and readable; the full 64-char digest is not needed at
    // the contract boundary because the project_id already disambiguates.
    let suffix = &digest[..24];
    Ok(format!("{}:{suffix}", kind.prefix()))
}

/// Validates that `id` is a well-formed opaque id of `expected_kind`.
pub fn validate_stable_id(id: &str, expected_kind: StableIdKind) -> Result<(), StableIdError> {
    let parts: Vec<&str> = id.split(':').collect();
    if parts.len() != 2 || parts[1].len() != 24 || !parts[1].chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(StableIdError::InvalidFormat);
    }
    match StableIdKind::from_prefix(id) {
        Some(actual) if actual == expected_kind => Ok(()),
        Some(_) => Err(StableIdError::WrongKind(expected_kind)),
        None => Err(StableIdError::InvalidFormat),
    }
}

/// The stable-id boundary check that production creation paths call before they
/// persist a scope binding. A binding reference that carries a recognised
/// `kind:` prefix is authoritative and MUST be a well-formed opaque stable id
/// (`validate_stable_id`); a malformed prefixed id (e.g. `vol:1`, `chap:3`) is
/// rejected outright — it is mutable display material leaking into the identity.
///
/// Bare, un-prefixed identifiers (the historical M0A fixture shape such as
/// `chapter-1`) are tolerated as a legacy migration form only. They carry no
/// identity prefix, so they cannot masquerade as a kind, and they are phased out
/// in B02 when the first real Chapter/Entity creation services land.
pub fn validate_scope_binding(id: &str) -> Result<(), StableIdError> {
    if id.contains(':') {
        // A prefixed reference must resolve to a real kind AND be well-formed.
        let kind = StableIdKind::from_prefix(id).ok_or(StableIdError::InvalidFormat)?;
        validate_stable_id(id, kind)
    } else {
        // Legacy bare id: tolerated in B01 for M0A fixtures, never a stable ref.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn child_seed(n: u8) -> String {
        // A permanent, immutable child identity: a UUID with a fixed byte for
        // determinism. uuid::Uuid is the seed, not a display field.
        Uuid::from_bytes([n; 16]).to_string()
    }

    #[test]
    fn mint_produces_a_unique_uuid_keyed_stable_id() {
        let a = mint(StableIdKind::Chapter).unwrap();
        let b = mint(StableIdKind::Chapter).unwrap();
        assert_ne!(a, b, "two mints must not collide");
        assert!(a.starts_with("chap:"), "id carries its kind prefix");
        assert!(validate_stable_id(&a, StableIdKind::Chapter).is_ok());
    }

    #[test]
    fn derive_is_deterministic_and_kind_bound() {
        let parent = mint(StableIdKind::Volume).unwrap();
        let seed = child_seed(1);
        let a = derive(StableIdKind::Chapter, &parent, &seed).unwrap();
        let b = derive(StableIdKind::Chapter, &parent, &seed).unwrap();
        assert_eq!(
            a, b,
            "same frozen parent + child id yields the identical id"
        );
        assert!(a.starts_with("chap:"));
        assert!(validate_stable_id(&a, StableIdKind::Chapter).is_ok());
        assert!(validate_stable_id(&a, StableIdKind::Volume).is_err());
    }

    #[test]
    fn derive_rejects_a_mutable_display_discriminator() {
        let parent = mint(StableIdKind::Volume).unwrap();
        // "vol:1:chap:3" is a display path/ordinal, not stable identity material.
        assert!(derive(StableIdKind::Chapter, &parent, "vol:1:chap:3").is_err());
        // A chapter-number-only seed is not a UUID, so it is mutable identity.
        assert!(derive(StableIdKind::Chapter, &parent, "3").is_err());
        assert!(derive(StableIdKind::Chapter, &parent, "chap-3").is_err());
    }

    #[test]
    fn derive_rejects_a_portable_display_parent() {
        // A parent that is itself a bare display path is rejected at the boundary.
        assert!(derive(StableIdKind::Chapter, "vol:1", &child_seed(1)).is_err());
        assert!(derive(StableIdKind::Chapter, "not-an-id", &child_seed(1)).is_err());
    }

    #[test]
    fn derive_rejects_same_kind_parent() {
        let parent = mint(StableIdKind::Chapter).unwrap();
        assert!(derive(StableIdKind::Chapter, &parent, &child_seed(1)).is_err());
    }

    #[test]
    fn validate_rejects_wrong_kind_and_malformed() {
        let id = mint(StableIdKind::Job).unwrap();
        assert!(validate_stable_id(&id, StableIdKind::Job).is_ok());
        assert!(validate_stable_id(&id, StableIdKind::Chapter).is_err());
        assert!(validate_stable_id("job:not-hex", StableIdKind::Job).is_err());
        assert!(validate_stable_id("no-colon", StableIdKind::Job).is_err());
    }
}

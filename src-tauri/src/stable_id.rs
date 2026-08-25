//! Opaque stable-ID boundary (B01-T04).
//!
//! Business entities (Book / Volume / Arc / Chapter / Entity / Job / ModelRun /
//! Decision) are identified by a durable opaque id that is derived from the
//! project id plus the entity's *stable* discriminator — a sequence number or a
//! business key that does not change when a title, chapter number or on-disk
//! path is edited. Rowids are never exposed across the contract boundary.
//!
//! The id is deterministic: rebuilding a project from its authoritative facts
//! reproduces the same id, so a stored id never has to be re-derived from a
//! mutable display field. The prefix encodes the entity kind so a mis-typed id
//! fails fast at the boundary rather than surfacing as a subtle data error.

use crate::hash::sha256_canonical;
use serde_json::json;

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
}

/// Deterministic opaque id for `kind` under `project_id`, keyed by a stable
/// `discriminator` (a sequence number or a durable business key). The id never
/// embeds a title, chapter number, or filesystem path, so it survives renames
/// and re-sequencing that are not themselves a retcon.
pub fn stable_id(
    kind: StableIdKind,
    project_id: &str,
    discriminator: &str,
) -> Result<String, String> {
    if project_id.trim().is_empty() || discriminator.trim().is_empty() {
        return Err("stable id requires a non-empty project id and discriminator".to_string());
    }
    let digest = sha256_canonical(&json!({
        "kind": kind.prefix(),
        "project": project_id,
        "discriminator": discriminator,
    }))
    .map_err(|error| error.to_string())?;
    // 16 hex chars is enough to be collision-safe for a single project while
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_is_deterministic_across_calls() {
        let a = stable_id(StableIdKind::Chapter, "p1", "vol:1:chap:3").unwrap();
        let b = stable_id(StableIdKind::Chapter, "p1", "vol:1:chap:3").unwrap();
        assert_eq!(a, b, "identical inputs must yield the identical id");
        assert!(a.starts_with("chap:"), "id carries its kind prefix");
    }

    #[test]
    fn stable_id_differs_when_project_or_discriminator_differs() {
        let base = stable_id(StableIdKind::Book, "p1", "1").unwrap();
        let other_project = stable_id(StableIdKind::Book, "p2", "1").unwrap();
        let other_disc = stable_id(StableIdKind::Book, "p1", "2").unwrap();
        assert_ne!(base, other_project);
        assert_ne!(base, other_disc);
    }

    #[test]
    fn stable_id_is_opaque_not_a_title_or_rowid() {
        let id = stable_id(StableIdKind::Volume, "p1", "5").unwrap();
        assert!(!id.contains("第5卷") && !id.contains("5"));
        assert_eq!(id.split(':').count(), 2, "kind + opaque payload only");
    }

    #[test]
    fn validate_rejects_wrong_kind_and_malformed() {
        let id = stable_id(StableIdKind::Job, "p1", "job-7").unwrap();
        assert!(validate_stable_id(&id, StableIdKind::Job).is_ok());
        assert!(validate_stable_id(&id, StableIdKind::Chapter).is_err());
        assert!(validate_stable_id("job:not-hex", StableIdKind::Job).is_err());
        assert!(validate_stable_id("no-colon", StableIdKind::Job).is_err());
    }
}

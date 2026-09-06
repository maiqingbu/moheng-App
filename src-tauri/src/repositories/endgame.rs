use crate::endgame::CompletionInput;
use crate::generated::types::ArtifactRef;
use crate::hash::sha256_canonical;
use crate::repositories::narrative::NarrativeRepository;
use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum EndgameRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("narrative: {0}")]
    Narrative(#[from] crate::repositories::narrative::NarrativeRepositoryError),
}

pub struct EndgameRepository;

#[derive(Debug, Clone)]
pub struct CompletionRead {
    pub input: CompletionInput,
    pub revision: i64,
    pub endgame_contract: Option<Value>,
    pub relevant_state_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionHistory {
    pub has_history: bool,
    pub currently_completed: bool,
}

impl EndgameRepository {
    pub fn mapping_workflow(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Value, EndgameRepositoryError> {
        let book_plan = Self::payload_for_head(connection, project_id, "BookPlan")?
            .unwrap_or_else(|| json!({}));
        let mysteries = book_plan.get("majorMysteryGoals").and_then(Value::as_array).into_iter().flatten().filter_map(|goal| {
            let id = goal.get("id").or_else(|| goal.get("mysteryId")).and_then(Value::as_str)?;
            Some(json!({"key":format!("mystery:{id}"),"id":id,"label":goal.get("description").or_else(|| goal.get("summary")).and_then(Value::as_str).unwrap_or("重要谜团")}))
        }).collect::<Vec<_>>();
        let reservations = book_plan.get("futureReservations").and_then(Value::as_array).into_iter().flatten().filter_map(|item| {
            let id = item.get("reservationId").and_then(Value::as_str)?;
            matches!(item.get("authorityLevel").and_then(Value::as_str), Some("A" | "B")).then(|| json!({"key":format!("reservation:{id}"),"id":id,"label":item.get("description").and_then(Value::as_str).unwrap_or("未来约束")}))
        }).collect::<Vec<_>>();
        let mut candidates = Vec::new();
        let mut statement = connection.prepare("SELECT a.artifact_id,COALESCE(c.title,'最终章节') FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version LEFT JOIN chapters c ON c.project_id=a.project_id AND c.final_artifact_id=a.artifact_id AND c.final_version=a.version WHERE h.project_id=?1 AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED' ORDER BY a.artifact_id")?;
        for row in statement.query_map([project_id], |row| Ok(json!({"ref":row.get::<_,String>(0)?,"label":row.get::<_,String>(1)?})))? { candidates.push(row?); }
        let mut hard_payoffs = Vec::new();
        let mut statement = connection.prepare("SELECT obligation_id,description FROM narrative_obligations WHERE project_id=?1 AND authority_level IN ('A','B') AND obligation_type='PAYOFF' AND lifecycle NOT IN ('RESOLVED','ABANDONED','SUPERSEDED') ORDER BY obligation_id")?;
        for row in statement.query_map([project_id], |row| Ok(json!({"key":format!("payoff:{}", row.get::<_,String>(0)?),"id":row.get::<_,String>(0)?,"label":row.get::<_,String>(1)?})))? { hard_payoffs.push(row?); }
        Ok(json!({"mysteries":mysteries,"reservations":reservations,"hardPayoffs":hard_payoffs,"fulfillmentCandidates":candidates}))
    }
    pub fn latest_head_ref(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
        scope_level: Option<&str>,
        scope_key: Option<&str>,
    ) -> Result<Option<ArtifactRef>, EndgameRepositoryError> {
        let row = match (scope_level, scope_key) {
            (Some(level), Some(key)) => connection
                .query_row(
                    "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 AND scope_level=?3 AND scope_key=?4",
                    libsql_rusqlite::params![project_id, artifact_type, level, key],
                    |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
                )
                .optional()?,
            (Some(level), None) => connection
                .query_row(
                    "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 AND scope_level=?3 ORDER BY version DESC, scope_key LIMIT 1",
                    libsql_rusqlite::params![project_id, artifact_type, level],
                    |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
                )
                .optional()?,
            _ => connection
                .query_row(
                    "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 ORDER BY version DESC, scope_key LIMIT 1",
                    libsql_rusqlite::params![project_id, artifact_type],
                    |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
                )
                .optional()?,
        };
        Ok(row)
    }

    pub fn completion_history(
        connection: &Connection,
        project_id: &str,
        current_digest: &str,
    ) -> Result<CompletionHistory, EndgameRepositoryError> {
        let report: Option<String> = connection
            .query_row(
                "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='BookCompletionReport' AND h.scope_level='PROJECT' ORDER BY h.version DESC, h.artifact_id DESC LIMIT 1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        let has_history = report.is_some()
            && connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM canonical_heads WHERE project_id=?1 AND artifact_type='BookExitState' AND scope_level='PROJECT') AND EXISTS(SELECT 1 FROM domain_receipts WHERE project_id=?1 AND domain='B14_BOOK_COMPLETION')",
                [project_id],
                |row| row.get(0),
            )?;
        let currently_completed = report
            .as_deref()
            .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
            .and_then(|payload| {
                payload
                    .get("relevantStateDigest")
                    .and_then(Value::as_str)
                    .map(|digest| digest == current_digest)
            })
            .unwrap_or(false)
            && has_history;
        Ok(CompletionHistory {
            has_history,
            currently_completed,
        })
    }

    pub fn latest_project_head_ref(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
    ) -> Result<Option<ArtifactRef>, EndgameRepositoryError> {
        Self::latest_head_ref(connection, project_id, artifact_type, Some("PROJECT"), None)
    }

    pub fn final_volume_exit_ref(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Option<ArtifactRef>, EndgameRepositoryError> {
        let volume_id: Option<String> = connection
            .query_row(
                "SELECT volume_id FROM volumes WHERE project_id=?1 ORDER BY volume_no DESC, volume_id DESC LIMIT 1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(volume_id) = volume_id else {
            return Ok(None);
        };
        connection
            .query_row(
                "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type='VolumeExitState' AND scope_level='VOLUME' AND json_extract(scope_key,'$.volumeId')=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn final_chapter_summary(
        connection: &Connection,
        project_id: &str,
    ) -> Result<(i64, Option<ArtifactRef>, Option<ArtifactRef>), EndgameRepositoryError> {
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND status='COMMITTED' AND final_artifact_id IS NOT NULL AND final_version IS NOT NULL",
            [project_id],
            |row| row.get(0),
        )?;
        let first = connection.query_row(
            "SELECT final_artifact_id,final_version,a.payload_hash FROM chapters c JOIN artifacts a ON a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version WHERE c.project_id=?1 AND c.status='COMMITTED' AND c.final_artifact_id IS NOT NULL AND c.final_version IS NOT NULL ORDER BY c.chapter_no LIMIT 1",
            [project_id],
            |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
        ).optional()?;
        let last = connection.query_row(
            "SELECT final_artifact_id,final_version,a.payload_hash FROM chapters c JOIN artifacts a ON a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version WHERE c.project_id=?1 AND c.status='COMMITTED' AND c.final_artifact_id IS NOT NULL AND c.final_version IS NOT NULL ORDER BY c.chapter_no DESC LIMIT 1",
            [project_id],
            |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
        ).optional()?;
        Ok((count, first, last))
    }

    fn payload_for_head(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
    ) -> Result<Option<Value>, EndgameRepositoryError> {
        connection
            .query_row(
                "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version JOIN artifacts a ON a.artifact_id=h.artifact_id AND a.version=h.version WHERE h.project_id=?1 AND h.artifact_type=?2 ORDER BY a.committed_at DESC, a.created_at DESC, h.artifact_id DESC LIMIT 1",
                libsql_rusqlite::params![project_id, artifact_type],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(Into::into)
    }

    fn ref_exists(
        connection: &Connection,
        project_id: &str,
        reference: &str,
    ) -> Result<bool, EndgameRepositoryError> {
        let bare = reference.split(':').skip(1).collect::<Vec<_>>().join(":");
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM artifacts WHERE project_id=?1 AND artifact_id IN (?2,?3)) OR EXISTS(SELECT 1 FROM narrative_obligations WHERE project_id=?1 AND obligation_id IN (?2,?3)) OR EXISTS(SELECT 1 FROM foreshadows WHERE project_id=?1 AND foreshadow_id IN (?2,?3)) OR EXISTS(SELECT 1 FROM state_deltas WHERE project_id=?1 AND state_id IN (?2,?3)) OR EXISTS(SELECT 1 FROM canon_evidence WHERE project_id=?1 AND evidence_id IN (?2,?3))",
            libsql_rusqlite::params![project_id, reference, bare],
            |row| row.get(0),
        )?)
    }

    fn mapping_refs_valid(
        connection: &Connection,
        project_id: &str,
        refs: Option<&Value>,
    ) -> Result<bool, EndgameRepositoryError> {
        let Some(refs) = refs.and_then(Value::as_array) else {
            return Ok(false);
        };
        if refs.is_empty() {
            return Ok(false);
        }
        for reference in refs.iter().filter_map(Value::as_str) {
            if !Self::ref_exists(connection, project_id, reference)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn fulfillment_evidence_refs_valid(
        connection: &Connection,
        project_id: &str,
        refs: Option<&Value>,
    ) -> Result<bool, EndgameRepositoryError> {
        let Some(refs) = refs.and_then(Value::as_array) else {
            return Ok(false);
        };
        if refs.is_empty() {
            return Ok(false);
        }
        for reference in refs.iter().filter_map(Value::as_str) {
            let artifact_id = reference.strip_prefix("final-chapter:").unwrap_or(reference);
            let current_final: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM artifacts a JOIN canonical_heads h ON h.project_id=a.project_id AND h.artifact_id=a.artifact_id AND h.version=a.version WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED')",
                libsql_rusqlite::params![project_id, artifact_id],
                |row| row.get(0),
            )?;
            if !current_final {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn mystery_count(
        connection: &Connection,
        project_id: &str,
        book_plan: Option<&Value>,
        contract: Option<&Value>,
    ) -> Result<i64, EndgameRepositoryError> {
        let goals = book_plan
            .and_then(|value| value.get("majorMysteryGoals"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mappings = contract
            .and_then(|value| value.get("mysteryMappings"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut count = 0;
        for goal in goals {
            let id = goal
                .get("id")
                .or_else(|| goal.get("mysteryId"))
                .and_then(Value::as_str);
            let mapping = id.and_then(|source| {
                mappings.iter().find(|item| {
                    item.get("sourceMysteryId").and_then(Value::as_str) == Some(source)
                })
            });
            let valid = mapping.is_some_and(|item| {
                item.get("mappingConfirmation").and_then(Value::as_str) == Some("USER_CONFIRMED")
            }) && Self::mapping_refs_valid(
                connection,
                project_id,
                mapping.and_then(|item| item.get("mappedResponsibilityRefs")),
            )?;
            if !valid {
                count += 1;
            }
        }
        Ok(count)
    }

    fn hard_payoff_count(
        connection: &Connection,
        project_id: &str,
        contract: Option<&Value>,
    ) -> Result<i64, EndgameRepositoryError> {
        let mappings = contract
            .and_then(|value| value.get("hardPayoffMappings"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut count = 0;
        for mapping in mappings {
            let fulfilled = mapping.get("fulfilled").and_then(Value::as_bool) == Some(true);
            let evidence_valid = Self::fulfillment_evidence_refs_valid(
                connection,
                project_id,
                mapping.get("fulfillmentEvidenceRefs"),
            )?;
            if !fulfilled || !evidence_valid {
                count += 1;
            }
        }
        Ok(count)
    }

    fn reservation_count(
        connection: &Connection,
        project_id: &str,
        book_plan: Option<&Value>,
        contract: Option<&Value>,
    ) -> Result<i64, EndgameRepositoryError> {
        let reservations = book_plan
            .and_then(|value| value.get("futureReservations"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mappings = contract
            .and_then(|value| value.get("reservationMappings"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut count = 0;
        for reservation in reservations {
            if !matches!(
                reservation.get("authorityLevel").and_then(Value::as_str),
                Some("A" | "B")
            ) {
                continue;
            }
            let id = reservation.get("reservationId").and_then(Value::as_str);
            let mapping = id.and_then(|source| {
                mappings
                    .iter()
                    .find(|item| item.get("reservationId").and_then(Value::as_str) == Some(source))
            });
            let valid = mapping.is_some_and(|item| {
                item.get("mappingConfirmation").and_then(Value::as_str) == Some("USER_CONFIRMED")
            }) && Self::fulfillment_evidence_refs_valid(
                connection,
                project_id,
                mapping.and_then(|item| item.get("fulfillmentEvidenceRefs")),
            )?;
            if !valid {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Combines the class declared by the authoritative VolumePlan with the
    /// B13 current-state reader. TaskProgress deliberately carries no class,
    /// so B14 must never infer one from a state delta payload.
    fn blocked_hard_tasks(
        connection: &Connection,
        project_id: &str,
    ) -> Result<i64, EndgameRepositoryError> {
        let mut classes = BTreeMap::new();
        let mut statement = connection.prepare(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='VolumePlan' AND h.scope_level='VOLUME' ORDER BY h.scope_key,h.artifact_id",
        )?;
        for row in statement.query_map([project_id], |row| row.get::<_, String>(0))? {
            let payload = serde_json::from_str::<Value>(&row?)?;
            for task in payload
                .get("tasks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (Some(task_id), Some(class)) = (
                    task.get("taskId").and_then(Value::as_str),
                    task.get("class").and_then(Value::as_str),
                ) else {
                    continue;
                };
                classes.insert(task_id.to_string(), class.to_string());
            }
        }
        let task_ids = classes.keys().map(String::as_str).collect::<Vec<_>>();
        let states = NarrativeRepository::task_states_by_ids_complete(
            connection,
            project_id,
            &task_ids,
        )?;
        let statuses = states
            .into_iter()
            .filter_map(|state| {
                state
                    .value
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|status| (state.state_id, status.to_string()))
            })
            .collect::<BTreeMap<_, _>>();
        Ok(classes
            .into_iter()
            .filter(|(task_id, class)| {
                matches!(class.as_str(), "REQUIRED" | "CLIMAX" | "RESERVED")
                    && matches!(
                        statuses.get(task_id).map(String::as_str).unwrap_or("NOT_STARTED"),
                        "NOT_STARTED" | "ACTIVE" | "BLOCKED"
                    )
            })
            .count() as i64)
    }

    fn relevant_refs(contract: Option<&Value>) -> Vec<String> {
        let mut refs = Vec::new();
        if let Some(value) = contract {
            refs.extend(
                value
                    .get("relevantRefs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned),
            );
            refs.extend(
                value
                    .get("hardResponsibilities")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .flat_map(|item| {
                        item.get("sourceRefs")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                    })
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned),
            );
            for (mapping_key, ref_keys) in [
                ("mysteryMappings", ["sourceMysteryId", "mappedResponsibilityRefs"].as_slice()),
                ("reservationMappings", ["reservationId", "fulfillmentEvidenceRefs"].as_slice()),
                ("hardPayoffMappings", ["payoffId", "fulfillmentEvidenceRefs"].as_slice()),
            ] {
                for mapping in value.get(mapping_key).and_then(Value::as_array).into_iter().flatten() {
                    for key in ref_keys {
                        match mapping.get(*key) {
                            Some(Value::String(reference)) => refs.push(reference.clone()),
                            Some(Value::Array(items)) => refs.extend(items.iter().filter_map(Value::as_str).map(ToOwned::to_owned)),
                            _ => {}
                        }
                    }
                }
            }
        }
        refs.sort();
        refs.dedup();
        refs
    }

    fn value_contains_ref(value: &Value, refs: &[String]) -> bool {
        match value {
            Value::String(item) => refs.iter().any(|reference| reference == item),
            Value::Array(items) => items.iter().any(|item| Self::value_contains_ref(item, refs)),
            Value::Object(items) => items.values().any(|item| Self::value_contains_ref(item, refs)),
            _ => false,
        }
    }

    fn relevant_change_pending(
        connection: &Connection,
        project_id: &str,
        refs: &[String],
    ) -> Result<(bool, bool), EndgameRepositoryError> {
        let mut relevant_change = false;
        let mut statement = connection.prepare(
            "SELECT scope,impact_seed_json FROM change_requests WHERE project_id=?1 AND status IN ('DRAFT','IMPACT_READY','PENDING_APPROVAL','APPROVED','APPLYING')",
        )?;
        for row in statement.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (scope, payload) = row?;
            let payload = serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null);
            if scope == "GLOBAL" || Self::value_contains_ref(&payload, refs) {
                relevant_change = true;
            }
        }
        let mut retcon = connection.prepare(
            "SELECT target_fact_json,downstream_impact_json FROM retcon_plans WHERE project_id=?1 AND status IN ('DRAFT','PENDING_APPROVAL','APPROVED')",
        )?;
        for row in retcon.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (target, downstream) = row?;
            let target = serde_json::from_str::<Value>(&target).unwrap_or(Value::Null);
            let downstream = serde_json::from_str::<Value>(&downstream).unwrap_or(Value::Null);
            if Self::value_contains_ref(&target, refs) || Self::value_contains_ref(&downstream, refs) {
                relevant_change = true;
            }
        }
        let mut stale = connection.prepare(
            "SELECT target_ref,revalidate_class FROM stale_queue WHERE project_id=?1 AND processed=0",
        )?;
        let mut relevant_stale = false;
        for row in stale.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (target, class) = row?;
            if refs.iter().any(|reference| reference == &target)
                || (refs.is_empty() && class == "REWRITE")
            {
                relevant_stale = true;
            }
        }
        Ok((relevant_change || relevant_stale, relevant_stale))
    }

    fn rich_relevant_state_digest(
        connection: &Connection,
        project_id: &str,
        input: &CompletionInput,
        book_plan: Option<&Value>,
        contract: Option<&Value>,
    ) -> Result<String, EndgameRepositoryError> {
        let (final_count, first, last) = Self::final_chapter_summary(connection, project_id)?;
        let mut obligations = Vec::new();
        let mut statement = connection.prepare(
            "SELECT obligation_id,authority_level,lifecycle,obligation_type FROM narrative_obligations WHERE project_id=?1 ORDER BY obligation_id",
        )?;
        for row in statement.query_map([project_id], |row| {
            Ok(json!({"id":row.get::<_, String>(0)?,"authority":row.get::<_, String>(1)?,"lifecycle":row.get::<_, String>(2)?,"type":row.get::<_, String>(3)?}))
        })? {
            obligations.push(row?);
        }
        let mut task_reality = Vec::new();
        let mut tasks = connection.prepare(
            "SELECT state_id,value_json FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' AND NOT EXISTS (SELECT 1 FROM state_deltas newer WHERE newer.project_id=state_deltas.project_id AND newer.state_id=state_deltas.state_id AND newer.state_type=state_deltas.state_type AND (newer.updated_at > state_deltas.updated_at OR (newer.updated_at=state_deltas.updated_at AND newer.rowid>state_deltas.rowid))) ORDER BY state_id",
        )?;
        for row in tasks.query_map([project_id], |row| {
            let value: String = row.get(1)?;
            Ok(json!({"id":row.get::<_, String>(0)?,"value":serde_json::from_str::<Value>(&value).unwrap_or(Value::Null)}))
        })? {
            task_reality.push(row?);
        }
        let control = contract.map(|value| {
            json!({
                "requestedEnding": value.get("requestedEnding"),
                "endingBoundaryDigest": value.get("endingBoundaryDigest"),
                "mysteryMappings": value.get("mysteryMappings"),
                "hardPayoffMappings": value.get("hardPayoffMappings"),
                "reservationMappings": value.get("reservationMappings"),
                "relevantRefs": value.get("relevantRefs")
            })
        });
        let mut input_json = serde_json::to_value(input)?;
        if let Some(object) = input_json.as_object_mut() {
            object.remove("endgameContractFresh");
        }
        sha256_canonical(&json!({
            "input": input_json,
            "bookPlan": book_plan,
            "control": control,
            "obligations": obligations,
            "taskReality": task_reality,
            "finalChapterInventory": {"count": final_count, "first": first, "last": last}
        }))
        .map_err(Into::into)
    }

    pub fn domain_digests(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Value, EndgameRepositoryError> {
        let mut canon = Vec::new();
        let mut statement = connection.prepare("SELECT fact_id,subject_id,predicate,object_json,valid_from_chapter_id,valid_to_chapter_id,source_artifact_id,source_version,authority_level FROM canon_facts WHERE project_id=?1 ORDER BY fact_id")?;
        for row in statement.query_map([project_id], |row| Ok(json!({"factId":row.get::<_,String>(0)?,"subjectId":row.get::<_,String>(1)?,"predicate":row.get::<_,String>(2)?,"objectJson":serde_json::from_str::<Value>(&row.get::<_,String>(3)?).unwrap_or(Value::Null),"validFrom":row.get::<_,Option<String>>(4)?,"validTo":row.get::<_,Option<String>>(5)?,"sourceArtifactId":row.get::<_,Option<String>>(6)?,"sourceVersion":row.get::<_,Option<i64>>(7)?,"authority":row.get::<_,String>(8)?})))? { canon.push(row?); }
        let mut knowledge = Vec::new();
        let mut statement = connection.prepare("SELECT knowledge_id,knower_id,fact_id,epistemic_state,visibility_scope,from_chapter_id,until_chapter_id FROM canon_knowledge WHERE project_id=?1 ORDER BY knowledge_id")?;
        for row in statement.query_map([project_id], |row| Ok(json!({"knowledgeId":row.get::<_,String>(0)?,"knowerId":row.get::<_,String>(1)?,"factId":row.get::<_,Option<String>>(2)?,"epistemicState":row.get::<_,String>(3)?,"visibilityScope":row.get::<_,Option<String>>(4)?,"fromChapterId":row.get::<_,Option<String>>(5)?,"untilChapterId":row.get::<_,Option<String>>(6)?})))? { knowledge.push(row?); }
        let mut continuity = Vec::new();
        let mut statement = connection.prepare("SELECT obligation_id,authority_level,lifecycle,obligation_type FROM narrative_obligations WHERE project_id=?1 ORDER BY obligation_id")?;
        for row in statement.query_map([project_id], |row| Ok(json!({"id":row.get::<_,String>(0)?,"authority":row.get::<_,String>(1)?,"lifecycle":row.get::<_,String>(2)?,"type":row.get::<_,String>(3)?})))? { continuity.push(row?); }
        let reservation = Self::payload_for_head(connection, project_id, "BookPlan")?
            .and_then(|value| value.get("futureReservations").cloned())
            .unwrap_or_else(|| json!([]));
        let obligations = continuity.clone();
        let mut tasks = Vec::new();
        let mut statement = connection.prepare("SELECT state_id,value_json FROM state_deltas s WHERE project_id=?1 AND state_type='volume.task.progress' AND NOT EXISTS (SELECT 1 FROM state_deltas newer WHERE newer.project_id=s.project_id AND newer.state_id=s.state_id AND newer.state_type=s.state_type AND (newer.updated_at > s.updated_at OR (newer.updated_at=s.updated_at AND newer.rowid>s.rowid))) ORDER BY state_id")?;
        for row in statement.query_map([project_id], |row| Ok(json!({"id":row.get::<_,String>(0)?,"value":serde_json::from_str::<Value>(&row.get::<_,String>(1)?).unwrap_or(Value::Null)})))? { tasks.push(row?); }
        let mut chapters = Vec::new();
        let mut statement = connection.prepare("SELECT c.chapter_id,c.chapter_no,c.final_artifact_id,c.final_version,a.payload_hash FROM chapters c LEFT JOIN artifacts a ON a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version WHERE c.project_id=?1 ORDER BY c.chapter_no,c.chapter_id")?;
        for row in statement.query_map([project_id], |row| Ok(json!({"chapterId":row.get::<_,String>(0)?,"chapterNo":row.get::<_,i64>(1)?,"finalArtifactId":row.get::<_,Option<String>>(2)?,"finalVersion":row.get::<_,Option<i64>>(3)?,"payloadHash":row.get::<_,Option<String>>(4)?})))? { chapters.push(row?); }
        Ok(json!({
            "canonDigest": sha256_canonical(&json!(canon))?,
            "knowledgeDigest": sha256_canonical(&json!(knowledge))?,
            "continuityDigest": sha256_canonical(&json!(continuity))?,
            "reservationDigest": sha256_canonical(&reservation)?,
            "obligationDigest": sha256_canonical(&json!(obligations))?,
            "taskRealityDigest": sha256_canonical(&json!(tasks))?,
            "finalChapterInventoryDigest": sha256_canonical(&json!(chapters))?,
            "manuscriptInventoryDigest": sha256_canonical(&json!(chapters))?
        }))
    }

    pub fn read_completion_input(
        connection: &Connection,
        project_id: &str,
    ) -> Result<CompletionRead, EndgameRepositoryError> {
        Self::read_completion_input_with_contract(connection, project_id, None)
    }

    pub fn read_completion_input_with_contract(
        connection: &Connection,
        project_id: &str,
        contract_override: Option<&Value>,
    ) -> Result<CompletionRead, EndgameRepositoryError> {
        let revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1 AND maintenance_state='READY'",
            [project_id],
            |row| row.get(0),
        )?;

        let endgame_contract = contract_override.cloned().or(Self::payload_for_head(
            connection,
            project_id,
            "EndgameContract",
        )?);
        let book_plan = Self::payload_for_head(connection, project_id, "BookPlan")?;

        let unresolved_hard_obligations: i64 = connection.query_row(
            "SELECT COUNT(*) FROM narrative_obligations WHERE project_id=?1 AND authority_level IN ('A','B') AND lifecycle NOT IN ('RESOLVED','ABANDONED','SUPERSEDED')",
            [project_id],
            |row| row.get(0),
        )?;
        let unresolved_hard_payoffs = Self::hard_payoff_count(
            connection,
            project_id,
            endgame_contract.as_ref(),
        )?;
        let continuity_threads = NarrativeRepository::continuity_threads_complete(connection, project_id)?;
        let open_hard_continuity = continuity_threads
            .iter()
            .filter(|thread| {
                matches!(thread.authority_level.as_str(), "A" | "B")
                    && matches!(thread.logical_status.as_str(), "OPEN" | "SUSPENDED")
            })
            .count() as i64;
        let continuity_advisories = continuity_threads
            .iter()
            .filter(|thread| {
                thread.authority_level == "C"
                    && matches!(thread.logical_status.as_str(), "OPEN" | "SUSPENDED")
            })
            .count() as i64;
        let blocked_required_tasks = Self::blocked_hard_tasks(connection, project_id)?;
        let (pending_relevant_change, plan_stale) = Self::relevant_change_pending(
            connection,
            project_id,
            &Self::relevant_refs(endgame_contract.as_ref()),
        )?;

        let final_volume: Option<(String, String)> = connection
            .query_row(
                "SELECT volume_id,status FROM volumes WHERE project_id=?1 ORDER BY volume_no DESC, volume_id DESC LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let final_volume_closed = final_volume
            .as_ref()
            .is_some_and(|(_, status)| status == "CLOSED");
        let volume_exit_state_available =
            Self::final_volume_exit_ref(connection, project_id)?.is_some();

        let export_integrity_failures: i64 = connection.query_row(
            "SELECT COUNT(*) FROM chapters c WHERE c.project_id=?1 AND (c.status!='COMMITTED' OR c.final_artifact_id IS NULL OR c.final_version IS NULL OR NOT EXISTS(SELECT 1 FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version JOIN canonical_heads h ON h.project_id=a.project_id AND h.artifact_id=a.artifact_id AND h.version=a.version WHERE a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED' AND a.payload_hash=p.payload_hash) OR NOT EXISTS(SELECT 1 FROM artifacts a JOIN commit_receipts r ON r.project_id=a.project_id AND r.command_id=json_extract(a.provenance_json,'$.commandId') WHERE a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version))",
            [project_id],
            |row| row.get(0),
        )?;

        let requested_ending_confirmed = endgame_contract
            .as_ref()
            .and_then(|payload| payload.pointer("/requestedEnding/confirmationSource"))
            .and_then(Value::as_str)
            == Some("USER_CONFIRMED");

        let unmapped_mysteries = Self::mystery_count(
            connection,
            project_id,
            book_plan.as_ref(),
            endgame_contract.as_ref(),
        )?;
        let unfulfilled_hard_reservations = Self::reservation_count(
            connection,
            project_id,
            book_plan.as_ref(),
            endgame_contract.as_ref(),
        )?;

        let mut input = CompletionInput {
            source_available: true,
            requested_ending_confirmed,
            endgame_contract_fresh: false,
            unmapped_mysteries,
            unresolved_hard_obligations,
            unresolved_hard_payoffs,
            unfulfilled_hard_reservations,
            open_hard_continuity,
            blocked_required_tasks,
            final_volume_closed,
            volume_exit_state_available,
            pending_relevant_change,
            plan_stale,
            export_integrity_failures,
            soft_advisories: continuity_advisories,
            detail_limit: 30,
        };
        let current_digest = Self::rich_relevant_state_digest(
            connection,
            project_id,
            &input,
            book_plan.as_ref(),
            endgame_contract.as_ref(),
        )?;
        input.endgame_contract_fresh = endgame_contract
            .as_ref()
            .and_then(|payload| payload.get("compiledFromRelevantStateDigest"))
            .and_then(Value::as_str)
            == Some(current_digest.as_str());
        Ok(CompletionRead {
            input,
            revision,
            endgame_contract,
            relevant_state_digest: current_digest,
        })
    }
}

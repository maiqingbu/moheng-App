import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

export function inspectB10Architecture(projectRoot = root) {
  const readFrom = relative => fs.readFileSync(path.join(projectRoot, relative), 'utf8')
  const creativeRust = readFrom('src-tauri/src/creative_skills.rs')
  const creativeTs = readFrom('src/production/skills/runtime.ts')
  const production = readFrom('src-tauri/src/production.rs')
  const commands = readFrom('src-tauri/src/commands.rs')
  const repair = readFrom('src-tauri/src/repair.rs')
  const harness = readFrom('src-tauri/src/harness/mod.rs')
  const projection = readFrom('src-tauri/src/repositories/production.rs')
  const ui = readFrom('src/ui/pages/ProductionCenter.vue')
  const registry = JSON.parse(readFrom('creative-skills/v1.2.0/registry.json'))

  const source = `${creativeRust}\n${creativeTs}`
  const sqlOutsideCreative = /use\s+libsql_rusqlite::Connection|\.query_row\(|\.execute\(|execute_batch\(|\bSELECT\s+|\bINSERT\s+|\bUPDATE\s+|\bDELETE\s+/i.test(source)
  const directProvider = /ProviderAdapter|OpenAI|Anthropic|fetch\s*\(|axios|reqwest|http::/i.test(source)
  const futureOwner = /B11|B12|B13|B14|B15|RC1|EndgameOwner|CompletionGate|NarrativeRuntimeDelta|CommitBundleV3/i.test(source)
  const secondProductionPath = /SkillProductionEngine|CreativeProductionEngine|NarrativeProductionEngine|production_start_legacy|legacyProduction/i.test(`${production}\n${commands}`)
  const all45TextEager = /skills?\.map\([^)]*SKILL\.md|readFileSync\([^)]*SKILL\.md|eager\s*:\s*true/i.test(`${creativeRust}\n${creativeTs}`)
  const skillFlags = registry.skills.length === 45
    && registry.skills.every(skill => skill.version === '1.2.0' && skill.authorityAccess === 'NONE_DIRECT' && skill.hardGateAuthority === false && skill.controlDecisionAuthority === false)
  const gates = {
    packageMetadata: registry.version === '1.2.0' && registry.skillCount === 45 && skillFlags,
    skillNoDirectPersistence: !sqlOutsideCreative,
    skillNoDirectProvider: !directProvider,
    oneProductionRoot: !secondProductionPath && /run_chapter_production\(/.test(production) && /crate::production::run_chapter_production/.test(commands),
    progressiveLoading: /import\.meta\.glob/.test(creativeTs) && !all45TextEager,
    boundedContext: /MAX_CONTEXT_BYTES|MAX_REFS|MAX_CAPSULES/.test(`${creativeRust}\n${creativeTs}`),
    candidateOnlyRepair: /candidate_status|SKILL_CANDIDATE_ONLY|CandidatePatch/.test(`${creativeRust}\n${creativeTs}`),
    hardQualitySeparation: /enforce_hard_quality_separation|enforceHardQualitySeparation/.test(`${creativeRust}\n${creativeTs}`) && /FinalChapter/.test(production),
    providerThroughRunner: /DurableModelRunner/.test(production) && !directProvider,
    noFutureOwner: !futureOwner,
    uiFromProjection: /query_production_center|creativeSkills|quality|skillsEnabled/.test(`${projection}\n${ui}`),
    noCommandsPersistenceSql: !/\bSELECT\s+|\bINSERT\s+|\bUPDATE\s+|\bDELETE\s+/.test(commands),
    noProductionPersistenceSql: !/\bSELECT\s+|\bINSERT\s+|\bUPDATE\s+|\bDELETE\s+/.test(production),
    repairOwnerRemainsExplicit: /TypedRepairOwner|repair_authorize|repair_execute/.test(repair),
    formalSkillLoader: /enrich_chapter_with_policy/.test(production) && /SkillBundleLoader::tracked_package/.test(creativeRust) && /load_selected/.test(creativeRust),
    formalMethodConsumed: /enrich_chapter_with_policy/.test(production) && /build_method_guidance/.test(creativeRust) && /method_guidance/.test(creativeRust) && /loaded_skill_texts/.test(creativeRust),
    productionRepairIntegration: /creative_skills::repair_candidate/.test(production) && /creative_skills::recheck_affected_dimensions/.test(production),
    semanticQualityEvaluator: /count_any|duplicate_sentence_count|dialogue_count|repeated_sentences/.test(creativeRust) && !/length_score/.test(creativeRust),
  }
  const globalGates = {
    second_authority: !/SecondAuthority|secondAuthorityOwner|AuthorityV2/i.test(source),
    second_production_path: !secondProductionPath,
    commit_bundle_v3: !/CommitBundleV3/i.test(source),
    narrative_runtime_delta: !/NarrativeRuntimeDelta/i.test(source),
    sql_outside_approved_owner: !sqlOutsideCreative && !/\bSELECT\s+|\bINSERT\s+|\bUPDATE\s+|\bDELETE\s+/.test(production),
    skill_direct_db_query: !sqlOutsideCreative,
    harness_arbitrary_write: !/Command::new|std::process::Command|git\s+(?:reset|checkout|commit)|\b(?:INSERT|UPDATE|DELETE)\s+FROM/i.test(harness),
    p3_permanent_authority: !/Skill[^\n]*(?:P3|AUTHORIZATION|consume)/i.test(source),
    future_reservation_bypass: !/Skill[^\n]*futureReservation|bypass[^\n]*reservation/i.test(source),
    plan_writes_reality: !/Skill[^\n]*(?:TASK_REALITY_WRITE|write.*reality)/i.test(source),
    candidate_visible_as_final: !/Skill[^\n]*(?:FinalChapter|canonical head)/i.test(source),
    ui_local_truth: /query_production_center|creativeSkills|quality/.test(`${projection}\n${ui}`),
    unbounded_project_context: /SKILL_CONTEXT_UNBOUNDED_INPUT/.test(source) && /MAX_CONTEXT_BYTES/.test(source),
    fake_provider_demo_formal_path: !/browser-demo|mockProduction|demoPort/.test(production),
    future_batch_owner_early: !/B11|B12|B13|B14|B15|RC1|EndgameOwner|CompletionGate/i.test(source),
  }
  const missingGates = Object.entries(gates).filter(([, passed]) => !passed).map(([name]) => name)
  const missingGlobalGates = Object.entries(globalGates).filter(([, passed]) => !passed).map(([name]) => name)
  const allMissingGates = [...missingGates, ...missingGlobalGates]
  return {
    schema: 'b10-architecture-r1',
    status: allMissingGates.length === 0 ? 'PASS' : 'FAIL',
    missingGates: allMissingGates,
    gates,
    globalGates,
    registry: { version: registry.version, skillCount: registry.skillCount },
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = inspectB10Architecture()
  console.log(JSON.stringify(result, null, 2))
  if (result.status !== 'PASS') process.exitCode = 1
}

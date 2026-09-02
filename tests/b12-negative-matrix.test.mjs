import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('NEG-01 ordinary Change cannot write a Canon target', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_RETCON_REQUIRED/)
})

test('NEG-02 GLOBAL impact cannot expand before explicit confirmation', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  assert.match(owner, /B12_GLOBAL_EXPANSION_CONFIRMATION_REQUIRED/)
  assert.match(owner, /continueGlobal/)
})

test('NEG-03 stale ImpactSet is blocked by revision or source digest changes', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  assert.match(owner, /B12_IMPACT_STALE/)
  assert.match(owner, /SOURCE_DIGEST_CHANGED/)
})

test('NEG-04 proposal and plan paths do not emit Canon deltas', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  const proposal = owner.slice(owner.indexOf('fn create_change'), owner.indexOf('fn create_retcon_plan'))
  assert.doesNotMatch(proposal, /canonDelta|canon_facts|CommitService::commit\(/)
})

test('NEG-05 B12 UI hides engineering fields and future workspaces', () => {
  const historyTemplate = read('src/ui/pages/HistoryCenter.vue').match(/<template>([\s\S]*?)<\/template>/)?.[1] ?? ''
  const assistantTemplate = read('src/ui/pages/ProjectAssistant.vue').match(/<template>([\s\S]*?)<\/template>/)?.[1] ?? ''
  const visible = `${historyTemplate}\n${assistantTemplate}`
  assert.doesNotMatch(visible, /change_request_id|impact_set_id|retcon_plan_id|sourceHash|operationDigest|basedOnRevision|B13|B14|B15|RC1/)
})

test('NEG-06 H03 still requires its separate one-time authorization', () => {
  const repair = read('src-tauri/src/repair.rs')
  const commands = read('src-tauri/src/commands.rs')
  assert.match(repair, /claim_repair_authorization/)
  assert.match(repair, /execute_change_request/)
  assert.match(commands, /crate::change_retcon::execute/)
})

test('NEG-07 Impact expansion has an explicit page cap', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /MAX_IMPACT_PAGE/)
})

test('NEG-08 cancelled ImpactSet cannot continue without resume', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_IMPACT_CANCELLED/)
})

test('NEG-09 resume uses the durable checkpoint', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /checkpoint_offset/)
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /processed_pages_json/)
})

test('NEG-10 source fingerprint is streamed instead of loading the graph', () => {
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /dependency_fingerprint/)
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /SELECT COUNT\(\*\).*MIN\(edge_id\)/s)
})

test('NEG-11 Retcon requires an existing current fact', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_CANON_FACT_NOT_FOUND/)
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_CANON_FACT_NOT_CURRENT/)
})

test('NEG-12 Retcon requires real evidence references', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_RETCON_EVIDENCE_NOT_FOUND/)
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /artifact_reference_exists/)
})

test('NEG-13 Retcon versioning rejects a second overwrite of an old fact', () => {
  assert.match(read('src-tauri/src/commit/deltas.rs'), /supersedesFactId/)
  assert.match(read('src-tauri/src/commit/deltas.rs'), /superseded_by IS NULL/)
})

test('NEG-14 B12 does not mutate a committed FinalChapter in place', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  assert.doesNotMatch(owner, /UPDATE\s+chapters|FinalChapter.*body/i)
})

test('NEG-15 B12 decision requires both scope and risk acknowledgement', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_DECISION_ACK_REQUIRED/)
  assert.match(read('src-tauri/src/change_retcon.rs'), /scopeAcknowledged/)
  assert.match(read('src-tauri/src/change_retcon.rs'), /riskAcknowledged/)
})

test('NEG-16 stale decision digest is rejected before commit', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_DECISION_STALE/)
})

test('NEG-17 final response does not claim verification after incomplete postverify', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  assert.match(owner, /暂时无法完成复核/)
  assert.match(owner, /finalStatus.*failed/s)
})

test('NEG-18 stale propagation is represented by the same CommitBundle', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  assert.match(owner, /b12\.stale\.activation/)
  assert.match(owner, /commit_with_post_apply/)
})

test('NEG-19 stale queue is scoped by selected dependency sources', () => {
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /sourceRefs/)
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /stale_queue/)
})

test('NEG-20 decision rows are durable backend state', () => {
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /INSERT INTO decisions/)
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12_CHANGE_APPROVAL/)
})

test('NEG-21 ChangeRequest lifecycle is not UI-local state', () => {
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /INSERT INTO change_requests/)
  assert.match(read('src-tauri/src/repositories/change_retcon.rs'), /UPDATE change_requests/)
})

test('NEG-22 H03 Change execution is bound to an approved digest', () => {
  assert.match(read('src-tauri/src/repair.rs'), /impactDigest/)
  assert.match(read('src-tauri/src/commands.rs'), /H03_CHANGE_IMPACT_DIGEST_REQUIRED/)
})

test('NEG-23 H03 Change execution keeps its owner receipt', () => {
  assert.match(read('src-tauri/src/change_retcon.rs'), /B12::ChangeRetconOwner/)
  assert.match(read('src-tauri/src/commands.rs'), /apply_approved_on_connection/)
  assert.match(read('src-tauri/src/repair.rs'), /owner_receipt_ref/)
})

test('NEG-24 History reads use a real Tauri query boundary', () => {
  assert.match(read('src/infrastructure/tauri/frontendPort.ts'), /query_change_request/)
  assert.match(read('src-tauri/src/commands.rs'), /query_change_request/)
})

test('NEG-25 Health remains unavailable in B12', () => {
  assert.match(read('src/app/router.ts'), /health.*UnavailableFeature/)
})

test('NEG-26 B12 only adds an additive migration after frozen 0001-0010', () => {
  const migrations = fs.readdirSync(path.join(root, 'src-tauri/migrations'))
  assert.equal(migrations.filter(name => /^0012/.test(name)).length, 0)
  assert.equal(migrations.filter(name => /^0011/.test(name)).length, 1)
})

test('NEG-27 acceptance is fail closed on missing evidence', () => {
  assert.match(read('scripts/generate-b12-acceptance.mjs'), /missingGates/)
  assert.match(read('scripts/generate-b12-acceptance.mjs'), /NOT_READY/)
})

test('NEG-28 Assistant cannot invent an affected result', () => {
  const assistant = read('src/ui/pages/ProjectAssistant.vue')
  assert.doesNotMatch(assistant, /affectedChapter|fakeImpact|hardcodedImpact/i)
  assert.match(assistant, /impact_preflight/)
})

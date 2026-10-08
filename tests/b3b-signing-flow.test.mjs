import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import test from 'node:test'

const root = resolve(import.meta.dirname, '..')
const read = path => readFileSync(resolve(root, path), 'utf8')

test('signing evaluation uses durable model execution and separate confirmation', () => {
  const signing = read('src-tauri/src/signing.rs')
  const commands = read('src-tauri/src/commands.rs')
  assert.match(signing, /execute_production_model\(/)
  assert.match(signing, /SIGNING_EVALUATION_CAPABILITY/)
  assert.match(signing, /confirm_signing_platform/)
  assert.match(commands, /"signing_confirm"\s*=>/)
  assert.match(signing, /UNKNOWN_REMOTE_OUTCOME_REQUIRES_DECISION/)
})

test('signing UI exposes platform matrix, exact quotes, and user confirmation', () => {
  const page = read('src/ui/pages/ProductionCenter.vue')
  const contracts = read('src/application/contracts.ts')
  assert.match(page, /platformMatrix/)
  assert.match(page, /exactQuote/)
  assert.match(page, /确认签约/)
  assert.match(page, /signing_confirm/)
  assert.match(page, /declarationPrompt/)
  assert.match(page, /confirmation\?\.confirmed === true/)
  assert.match(contracts, /\| 'signing_confirm'/)
})

test('legacy REVISE signing receipts are projected and labeled as failed', () => {
  const repository = read('src-tauri/src/repositories/production.rs')
  const labels = read('src/ui/presentation/projectLabels.ts')
  const page = read('src/ui/pages/ProductionCenter.vue')
  assert.match(repository, /let legacy_revise = signing_status == "REVISE"/)
  assert.match(repository, /if legacy_revise \{ "FAIL" \} else \{ signing_status \}/)
  assert.match(repository, /历史签约评估未通过（旧版 REVISE）/)
  assert.match(labels, /REVISE: '未通过（历史评估）'/)
  assert.match(page, /status === 'REVISE'\) return '未通过（历史评估）'/)
})

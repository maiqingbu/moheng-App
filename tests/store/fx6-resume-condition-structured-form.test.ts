/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from 'vitest'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import fs from 'node:fs'
import { createApp, h, nextTick, reactive } from 'vue'
import ResumeConditionFields from '../../src/ui/components/ResumeConditionFields.vue'
import { buildResponsibilityEvidence, serializeResumeCondition } from '../../src/ui/flows/resumeCondition'
import { productErrorMessage } from '../../src/ui/presentation/productLanguage'

const testDirectory = dirname(fileURLToPath(import.meta.url))

const validCases = [
  [{ kind: 'NEXT_MEETING', entityA: 'entity:linzhou', entityB: 'entity:guqing', value: '' }, { kind: 'NEXT_MEETING', value: { entityA: 'entity:linzhou', entityB: 'entity:guqing' } }],
  [{ kind: 'ENTITY_PRESENT', entityA: '', entityB: '', value: 'entity:linzhou' }, { kind: 'ENTITY_PRESENT', value: 'entity:linzhou' }],
  [{ kind: 'KNOWLEDGE_ACQUIRED', entityA: '', entityB: '', value: 'knowledge:seal' }, { kind: 'KNOWLEDGE_ACQUIRED', value: 'knowledge:seal' }],
  [{ kind: 'TASK_SATISFIED', entityA: '', entityB: '', value: 'task:promise' }, { kind: 'TASK_SATISFIED', value: 'task:promise' }],
  [{ kind: 'AFTER_CHAPTER', entityA: '', entityB: '', value: '12' }, { kind: 'AFTER_CHAPTER', value: 12 }],
  [{ kind: 'IN_VOLUME', entityA: '', entityB: '', value: 'volume:2' }, { kind: 'IN_VOLUME', value: 'volume:2' }],
] as const

function mountFields() {
  const form = reactive({ value: { kind: 'ENTITY_PRESENT', entityA: '', entityB: '', value: '' } })
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp({
    render: () => h(ResumeConditionFields, {
      modelValue: form.value,
      'onUpdate:modelValue': value => { form.value = value },
    }),
  })
  app.mount(container)
  return { app, container, form }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('FX-6 structured resume condition form', () => {
  it.each(validCases)('serializes each supported condition into the backend shape', (draft, expected) => {
    expect(serializeResumeCondition(draft)).toEqual(expected)
  })

  it('fx6-resume-condition-structured-form selects NEXT_MEETING and submits both entities', async () => {
    const mounted = mountFields()
    const select = mounted.container.querySelector('select')!
    select.value = 'NEXT_MEETING'
    select.dispatchEvent(new Event('change', { bubbles: true }))
    await nextTick()
    const inputs = [...mounted.container.querySelectorAll('input')]
    inputs[0].value = 'entity:linzhou'
    inputs[0].dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    inputs[1].value = 'entity:guqing'
    inputs[1].dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    expect(serializeResumeCondition(mounted.form.value)).toEqual({
      kind: 'NEXT_MEETING',
      value: { entityA: 'entity:linzhou', entityB: 'entity:guqing' },
    })
    mounted.app.unmount()
  })

  it('keeps raw condition JSON behind Advanced Mode and offers a quote-only ordinary evidence input', () => {
    const page = fs.readFileSync(resolve(testDirectory, '../../src/ui/pages/ProductionCenter.vue'), 'utf8')
    expect(page).toContain('ResumeConditionFields')
    expect(page).toContain('v-if="store.advancedMode"')
    expect(page).toContain('原始兑现条件 JSON')
    expect(page).toContain('普通模式只需粘贴原文引文')
    expect(page).toContain("type: 'thread_responsibility_adjust'")
    expect(productErrorMessage('EVIDENCE_QUOTE_NOT_UNIQUE')).toContain('更长')
  })

  it('sends only a quote in ordinary mode and preserves explicit character ranges in Advanced Mode', () => {
    expect(buildResponsibilityEvidence(' 林舟推开石门 ')).toEqual({ quote: '林舟推开石门' })
    expect(buildResponsibilityEvidence('林舟推开石门', 0, 6, true)).toEqual({
      start: 0,
      end: 6,
      quote: '林舟推开石门',
    })
  })
})

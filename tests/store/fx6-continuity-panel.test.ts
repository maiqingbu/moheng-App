/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from 'vitest'
import { createApp, h } from 'vue'
import ContinuityPanel from '../../src/ui/components/ContinuityPanel.vue'

const threads = [
  {
    threadId: 'thread:obligation:door',
    threadClass: 'OBLIGATION',
    description: '林舟必须兑现门后的承诺。',
    fulfillmentObject: 'entity:linzhou',
    logicalStatus: 'OPEN',
    sourceEventRef: 'event:event:fact:opening-door',
    evidenceRefs: [{ artifactId: 'artifact:chapter:1', version: 1, rangeStart: 0, rangeEnd: 6, quote: '林舟推开石门' }],
    participantEntities: ['entity:linzhou'],
    resumeCondition: { kind: 'ENTITY_PRESENT', value: 'entity:linzhou' },
    authorityLevel: 'A',
    currentDisposition: 'ADVANCE',
    sourceRefs: [{ kind: 'event', ref: 'event:fact:opening-door' }],
  },
  {
    threadId: 'thread:mystery:letter',
    threadClass: 'MYSTERY',
    description: '顾清尚未解释信中的记号。',
    fulfillmentObject: 'entity:guqing',
    logicalStatus: 'SUSPENDED',
    sourceEventRef: 'event:event:fact:letter-mark',
    evidenceRefs: [{ artifactId: 'artifact:chapter:2', version: 3, rangeStart: 8, rangeEnd: 14, quote: '信上有暗纹' }],
    participantEntities: ['entity:guqing'],
    resumeCondition: { kind: 'NEXT_MEETING', value: { entityA: 'entity:linzhou', entityB: 'entity:guqing' } },
    authorityLevel: 'B',
    currentDisposition: 'HOLD',
    sourceRefs: [{ kind: 'event', ref: 'event:fact:letter-mark' }],
  },
  {
    threadId: 'thread:foreshadow:quiet',
    threadClass: 'FORESHADOW',
    description: '保留为普通候选。',
    fulfillmentObject: null,
    logicalStatus: 'OPEN',
    sourceEventRef: null,
    evidenceRefs: [],
    participantEntities: [],
    resumeCondition: { kind: 'IN_VOLUME', value: 'volume:2' },
    authorityLevel: 'C',
    currentDisposition: 'OBSERVE',
    sourceRefs: [],
  },
]

function mountPanel(advanced = false, truncated = false) {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp({
    render: () => h(ContinuityPanel, { threads, advanced, truncated }),
  })
  app.mount(container)
  return { app, container }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('FX-6 continuity panel', () => {
  it('fx6-continuity-panel-readonly renders open and suspended threads without direct status mutation controls', () => {
    const mounted = mountPanel()
    const html = mounted.container.innerHTML
    expect(mounted.container.textContent).toContain('未完成剧情线')
    expect(mounted.container.textContent).toContain('OPEN')
    expect(mounted.container.textContent).toContain('SUSPENDED')
    expect(mounted.container.textContent).toContain('A 级')
    expect(mounted.container.textContent).toContain('B 级')
    expect(mounted.container.textContent).toContain('林舟推开石门')
    expect(mounted.container.textContent).toContain('发起变更')
    expect(mounted.container.querySelectorAll('button')).toHaveLength(2)
    expect(html).not.toContain('thread_responsibility_adjust')
    expect(html).not.toContain('删除')
    expect(html).not.toContain('撤销')
    expect(html).not.toContain('event:event:fact:opening-door')
    expect(html).not.toContain('artifact:chapter:1')
    expect(html).toMatchSnapshot('continuity-panel-normal-mode')
    mounted.app.unmount()
  })

  it('fx6-continuity-panel-advanced-mode displays exact references for advanced users', () => {
    const mounted = mountPanel(true)
    const html = mounted.container.innerHTML
    expect(html).toContain('event:event:fact:opening-door')
    expect(html).toContain('artifact:chapter:1')
    expect(html).toContain('字符 0–6')
    expect(html).toMatchSnapshot('continuity-panel-advanced-mode')
    mounted.app.unmount()
  })

  it('fx6-continuity-truncated-notice tells the user that only part of the projection is shown', () => {
    const mounted = mountPanel(false, true)
    expect(mounted.container.textContent).toContain('仅显示部分，已截断')
    mounted.app.unmount()
  })
})

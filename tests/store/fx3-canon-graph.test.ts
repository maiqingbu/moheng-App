/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick } from 'vue'
import CanonGraph from '../../src/ui/pages/CanonGraph.vue'

const harness = vi.hoisted(() => ({ store: null as any }))

vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))

const characters = [
  { id: 'person-kappa', name: '甲', role: '主角', status: '正式', location: '城中', note: '' },
  { id: 'person-beta', name: '乙', role: '同伴', status: '正式', location: '山中', note: '' },
  { id: 'person-omega', name: '丙', role: '师长', status: '正式', location: '山中', note: '' },
  { id: 'person-alpha', name: '丁', role: '对手', status: '正式', location: '城中', note: '' },
  { id: 'person-delta', name: '戊', role: '证人', status: '正式', location: '城中', note: '' },
]

function mountGraph(people = characters) {
  harness.store = {
    canon: {
      characters: { items: people, nextCursor: null, total: people.length },
      relations: { items: [
        { from: 'person-kappa', to: 'person-beta', label: '信任', tone: 'positive' },
        { from: 'person-omega', to: 'person-alpha', label: '对立', tone: 'negative' },
        { from: 'person-kappa', to: 'missing-person', label: '悬空关系', tone: 'neutral' },
      ], nextCursor: null, total: 3 },
      timeline: { items: [], nextCursor: null, total: 0 },
      foreshadows: { items: [], nextCursor: null, total: 0 },
      knowledge: { items: [], nextCursor: null, total: 0 },
    },
    loadCanon: vi.fn(async () => undefined),
    commandState: vi.fn(() => ({ status: 'idle', message: '' })),
    runCommand: vi.fn(async () => undefined),
    project: { id: 'project:fx3' },
  }
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(CanonGraph)
  app.mount(container)
  return { app, container }
}

afterEach(() => { document.body.innerHTML = '' })

describe('FX-3 CanonGraph data-driven rendering', () => {
  it('fx3-canon-graph-data-driven renders arbitrary characters and only valid relation lines', async () => {
    const rendered = mountGraph()
    await nextTick()

    expect(rendered.container.querySelectorAll('.graph-node')).toHaveLength(5)
    expect(rendered.container.querySelectorAll('.graph-stage svg line')).toHaveLength(2)
    expect([...rendered.container.querySelectorAll('.graph-node')].map(node => node.textContent).join(' ')).not.toContain('shen')

    const positions = [...rendered.container.querySelectorAll<HTMLElement>('.graph-node')]
      .map(node => `${node.style.left},${node.style.top}`)
    expect(new Set(positions).size).toBe(5)
    expect(rendered.container.querySelectorAll('.graph-node.primary')).toHaveLength(1)
    rendered.app.unmount()
  })

  it('fx3-canon-graph-empty shows an empty state when there are no formal characters', async () => {
    const rendered = mountGraph([])
    await nextTick()

    expect(rendered.container.textContent).toContain('暂无正式人物')
    expect(rendered.container.querySelectorAll('.graph-node')).toHaveLength(0)
    expect(rendered.container.querySelectorAll('.graph-stage svg line')).toHaveLength(0)
    rendered.app.unmount()
  })

  it('fx3-canon-graph-layout is deterministic and assigns distinct coordinates', async () => {
    const { layoutCanonGraphNodes } = await vi.importActual<typeof import('../../src/ui/presentation/canonGraphLayout')>('../../src/ui/presentation/canonGraphLayout')
    const ids = ['person-epsilon', 'person-beta', 'person-alpha', 'person-gamma', 'person-delta']
    const first = layoutCanonGraphNodes(ids)
    const second = layoutCanonGraphNodes(ids)

    expect(first).toEqual(second)
    expect(new Set(Object.values(first).map(point => `${point.x},${point.y}`)).size).toBe(ids.length)
  })
})

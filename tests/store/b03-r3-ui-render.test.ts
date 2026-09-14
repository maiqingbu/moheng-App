/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import { createApp } from 'vue'
import VolumePlanningProgress from '../../src/ui/components/VolumePlanningProgress.vue'

const operation = (overrides: Record<string, unknown> = {}) => ({
  jobId: 'job:b03:r3',
  kind: 'B03::VOLUME_INTENT',
  phase: 'BUILD_VOLUME_GOAL',
  status: 'RUNNING',
  active: true,
  steps: [
    { id: 'done', label: '已完成', status: 'done' },
    { id: 'active', label: '处理中', status: 'active' },
    { id: 'pending', label: '等待中', status: 'pending' },
  ],
  ...overrides,
})

const render = (props: Record<string, unknown> = {}) => {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(VolumePlanningProgress, props)
  app.mount(container)
  return { app, container }
}

describe('B03-R3 rendered planning progress', () => {
  it('renders backend status classes for the VolumeManager progress component', () => {
    const rendered = render({ operation: operation(), inFlight: true })
    expect(rendered.container.querySelectorAll('.timeline-item.done')).toHaveLength(1)
    expect(rendered.container.querySelectorAll('.timeline-item.active')).toHaveLength(1)
    expect(rendered.container.querySelectorAll('.timeline-item.pending')).toHaveLength(1)
    expect(rendered.container.querySelector('.timeline-item.active .timeline-marker')?.textContent).toBe('…')
    rendered.app.unmount()
  })

  it('renders the same status classes for the VolumeWorkspace progress component', () => {
    const rendered = render({ operation: operation({ kind: 'B03::VOLUME_PLAN', phase: 'BUILD_TASK_ROUTE' }) })
    expect(rendered.container.querySelectorAll('.timeline-item.active')).toHaveLength(1)
    expect(rendered.container.querySelector('.timeline-item.active .timeline-marker')?.textContent).toBe('…')
    rendered.app.unmount()
  })

  it('renders no active item after completion and one failed item on failure', () => {
    const completed = render({
      operation: operation({ status: 'COMPLETED', active: false, steps: [
          { id: 'one', label: '完成一', status: 'done' },
          { id: 'two', label: '完成二', status: 'done' },
        ] }),
    })
    expect(completed.container.querySelectorAll('.timeline-item.active')).toHaveLength(0)
    expect(completed.container.querySelectorAll('.timeline-item.done')).toHaveLength(2)
    completed.app.unmount()

    const failed = render({
      operation: operation({ status: 'FAILED', active: false, steps: [
          { id: 'one', label: '完成一', status: 'done' },
          { id: 'two', label: '失败二', status: 'failed' },
          { id: 'three', label: '等待三', status: 'pending' },
        ] }),
    })
    expect(failed.container.querySelectorAll('.timeline-item.failed')).toHaveLength(1)
    expect(failed.container.querySelectorAll('.timeline-item.active')).toHaveLength(0)
    failed.app.unmount()
  })

  it('keeps Intent-to-Plan terminal badge truthful during refresh', () => {
    const intentWaiting = render({ operation: operation({ status: 'COMPLETED', active: false, steps: [] }), inFlight: true })
    expect(intentWaiting.container.textContent).toContain('正在等待规划阶段…')
    expect(intentWaiting.container.textContent).not.toContain('规划已完成')
    intentWaiting.app.unmount()

    const planSyncing = render({ operation: operation({ kind: 'B03::VOLUME_PLAN', status: 'COMPLETED', active: false, steps: [] }), inFlight: true })
    expect(planSyncing.container.textContent).toContain('正在同步规划结果…')
    expect(planSyncing.container.textContent).not.toContain('正在等待规划阶段…')
    planSyncing.app.unmount()

    const planActive = render({ operation: operation({ kind: 'B03::VOLUME_PLAN', status: 'RUNNING', active: true }) })
    expect(planActive.container.textContent).toContain('正在规划本卷任务路线')
    planActive.app.unmount()

    const final = render({ operation: operation({ kind: 'B03::VOLUME_PLAN', status: 'COMPLETED', active: false, steps: [] }), inFlight: false })
    expect(final.container.textContent).toContain('规划已完成')
    final.app.unmount()
  })
})

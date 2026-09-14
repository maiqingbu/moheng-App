import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = relative => readFileSync(new URL(`../${relative}`, import.meta.url), 'utf8')
const manager = read('src/ui/pages/VolumeManager.vue')
const workspace = read('src/ui/pages/VolumeWorkspace.vue')
const progress = read('src/ui/components/VolumePlanningProgress.vue')
const styles = read('src/ui/styles/pages.css')

test('B03-R2 VolumeManager and VolumeWorkspace bind backend step status to DOM classes', () => {
  assert.match(manager, /VolumePlanningProgress/)
  assert.match(workspace, /VolumePlanningProgress/)
  assert.match(progress, /class="timeline-item"\s+:class="step\.status"/)
  assert.match(progress, /stepMarker\(step\.status\)/)
})

test('B03-R2 active pulse selector is reachable and terminal steps have no active class', () => {
  assert.match(styles, /\.timeline-item\.active \.timeline-marker\s*\{[^}]*animation:opening-phase-pulse/)
  const renderedClasses = [
    { status: 'done' },
    { status: 'active' },
    { status: 'pending' },
  ].map(step => `timeline-item ${step.status}`)
  assert.equal(renderedClasses.filter(value => value.endsWith(' active')).length, 1)
  const terminalClasses = [{ status: 'done' }, { status: 'done' }].map(step => `timeline-item ${step.status}`)
  assert.equal(terminalClasses.filter(value => value.endsWith(' active')).length, 0)
})

test('B03-R2 Intent-to-Plan waiting badge never reports completed planning', () => {
  assert.match(progress, /props\.inFlight && operation\?\.kind === 'B03::VOLUME_PLAN' && !operation\.active && operation\.status === 'COMPLETED'/)
  assert.match(progress, /return '正在同步规划结果…'/)
  assert.match(progress, /return '正在等待规划阶段…'/)
  assert.match(progress, /return '规划已完成'/)
})

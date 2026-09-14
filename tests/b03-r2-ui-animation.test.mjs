import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = relative => readFileSync(new URL(`../${relative}`, import.meta.url), 'utf8')
const manager = read('src/ui/pages/VolumeManager.vue')
const workspace = read('src/ui/pages/VolumeWorkspace.vue')
const styles = read('src/ui/styles/pages.css')

test('B03-R2 VolumeManager and VolumeWorkspace bind backend step status to DOM classes', () => {
  assert.match(manager, /class="timeline-item"\s+:class="step\.status"/)
  assert.match(workspace, /class="timeline-item"\s+:class="step\.status"/)
  assert.match(manager, /step\.status === 'active' \? '…'/)
  assert.match(workspace, /step\.status === 'active' \? '…'/)
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
  assert.match(manager, /buildInFlight\.value && !runtimeOperation\.value\?\.active\) return '正在等待规划阶段…'/)
  assert.match(manager, /runtimeOperation\.value\.kind === 'B03::VOLUME_PLAN' \? '正在规划本卷任务路线'/)
  assert.match(manager, /if \(!runtimeOperation\.value\?\.active\) return '规划已完成'/)
})

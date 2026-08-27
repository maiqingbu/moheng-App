import { describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'

import type { CommandReceipt, CommandRequest } from '../../src/application/contracts'
import { BrowserDemoPort } from '../../src/infrastructure/browser-demo/demoPort'
import { TauriFrontendPort } from '../../src/infrastructure/tauri/frontendPort'

const receiptSemantics = (receipt: CommandReceipt) => ({
  projectId: receipt.projectId,
  resultProjectId: receipt.resultProjectId,
})

describe('B01 Receipt source/result parity', () => {
  it('CHAPTER-E2E-01: queryChapterFinal uses the production Tauri command and ChapterProjection DTO', async () => {
    const projection = {
      id: 'chapter-1', volumeId: 'volume-1', number: 1, title: '第一章',
      status: 'official', version: 'v1', wordCount: 2500, updatedAt: '2026-08-28T00:00:00Z', content: ['正文'],
    }
    const invoke = { invoke: vi.fn(async (command: string) => {
      expect(command).toBe('query_chapter_final')
      return projection
    }) }
    const tauri = new TauriFrontendPort(invoke)
    await expect(tauri.queryChapterFinal({ projectId: 'p1', scope: 'CHAPTER', chapterId: 'chapter-1', pageSize: 1 })).resolves.toEqual(projection)
    expect(invoke.invoke).toHaveBeenCalledWith('query_chapter_final', { request: { projectId: 'p1', scope: 'CHAPTER', chapterId: 'chapter-1', pageSize: 1 } })
  })

  it('RECEIPT-PARITY-CREATE: create uses the created project for the current Rust shape', async () => {
    const demo = new BrowserDemoPort()
    const receipt = await demo.executeCommand({
      type: 'project_create_from_idea',
      commandId: 'parity-create',
      idempotencyKey: 'parity-create-key',
      requestedAt: '2026-08-26T00:00:00Z',
      payload: { title: 'Parity create' },
    })
    expect(receipt.projectId).toBe(receipt.resultProjectId)
    expect(receipt.resultProjectId).toMatch(/^proj-/)
  })

  it('RECEIPT-PARITY-DUPLICATE: duplicate keeps source projectId and returns resultProjectId', async () => {
    const demo = new BrowserDemoPort()
    const created = await demo.executeCommand({
      type: 'project_create_from_idea',
      commandId: 'parity-duplicate-create',
      idempotencyKey: 'parity-duplicate-create-key',
      requestedAt: '2026-08-26T00:00:00Z',
      payload: { title: 'Parity duplicate' },
    })
    const source = created.resultProjectId as string
    const receipt = await demo.executeCommand({
      type: 'project_duplicate',
      commandId: 'parity-duplicate',
      idempotencyKey: 'parity-duplicate-key',
      projectId: source,
      expectedRevision: 0,
      requestedAt: '2026-08-26T00:00:01Z',
    })
    expect(receipt.projectId).toBe(source)
    expect(receipt.resultProjectId).toBeTruthy()
    expect(receipt.resultProjectId).not.toBe(source)
  })

  it('RECEIPT-PARITY-PROJECT-COMMAND: Demo and Tauri adapters expose the same source/result semantics', async () => {
    const rustShape = (request: CommandRequest): CommandReceipt => ({
      commandId: request.commandId as string,
      projectId: request.projectId,
      resultProjectId: request.type === 'project_duplicate' ? 'project-C' : request.projectId,
      acceptedAt: '2026-08-26T00:00:00Z',
      finalStatus: 'committed',
      message: 'ok',
      receiptId: 'receipt-1',
    })
    const invoke = { invoke: vi.fn(async (_command: string, payload: unknown) => rustShape((payload as { request: CommandRequest }).request)) }
    const tauri = new TauriFrontendPort(invoke)
    const request: CommandRequest = {
      type: 'project_rename',
      commandId: 'parity-project-command',
      idempotencyKey: 'parity-project-command-key',
      projectId: 'project-A',
      requestedAt: '2026-08-26T00:00:00Z',
      payload: { title: 'Parity renamed' },
    }
    const tauriReceipt = await tauri.executeCommand(request)
    expect(receiptSemantics(tauriReceipt)).toEqual({ projectId: 'project-A', resultProjectId: 'project-A' })

    const demo = new BrowserDemoPort()
    const created = await demo.executeCommand({
      type: 'project_create_from_idea',
      commandId: 'parity-command-create',
      idempotencyKey: 'parity-command-create-key',
      requestedAt: '2026-08-26T00:00:00Z',
      payload: { title: 'Parity command' },
    })
    const demoReceipt = await demo.executeCommand({
      ...request,
      projectId: created.resultProjectId,
    })
    expect(demoReceipt.projectId).toBe(created.resultProjectId)
    expect(demoReceipt.resultProjectId).toBe(created.resultProjectId)
    expect(tauriReceipt.projectId).toBe(request.projectId)
    expect(tauriReceipt.resultProjectId).toBe(tauriReceipt.projectId)
  })

  it('DUPLICATE-RESULT-02: the Library page navigates only to receipt.resultProjectId', () => {
    const page = readFileSync(new URL('../../src/ui/pages/ProjectLibrary.vue', import.meta.url), 'utf8')
    expect(page).toContain('const createdProjectId = receipt.resultProjectId')
    expect(page).not.toContain('const projectId = receipt.projectId')
  })

  it('LIBRARY-REV-03: degraded Library items disable every revision-backed destructive action', () => {
    const page = readFileSync(new URL('../../src/ui/pages/ProjectLibrary.vue', import.meta.url), 'utf8')
    expect(page).toContain(`:disabled="!item.operational" @click="askAction(item,'duplicate')"`)
    expect(page).toContain(`:disabled="!item.operational" @click="askAction(item,'archive')"`)
    expect(page).toContain(`:disabled="!item.operational" @click="askAction(item,'delete')"`)
    expect(page).toContain(':disabled="!item.operational" @click="restoreProject(item)"')
    expect(page).not.toContain('revision ?? 0')
  })
})

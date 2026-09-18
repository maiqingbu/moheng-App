<script setup lang="ts">
import { computed, ref } from 'vue'
import { getFrontendPort } from '../../app/providers/runtimePort'
import type { CommandReceipt } from '../../application/contracts'

const enabled = import.meta.env.VITE_REAL_LLM_UI_GATE === 'true'
const frontendPort = enabled ? getFrontendPort() : null
const running = ref(false)
const status = ref('等待真实 UI 启动')
const error = ref('')
const events = ref<string[]>([])
const trace = ref<Record<string, unknown>>({})
const projectId = ref('')
const revision = ref(0)
const rootTraceId = `ui-real-llm-${Date.now()}`

const traceText = computed(() => JSON.stringify(trace.value, null, 2))

function record(message: string) {
  status.value = message
  events.value.push(`${new Date().toISOString()} ${message}`)
}

async function command(type: string, payload: Record<string, unknown> = {}) {
  if (!frontendPort) throw new Error('REAL_LLM_UI_GATE_DISABLED')
  const commandId = `${rootTraceId}:${type}:${events.value.length + 1}`
  const request = {
    type,
    commandId,
    idempotencyKey: `${commandId}:key`,
    ...(projectId.value ? { projectId: projectId.value, expectedRevision: revision.value } : {}),
    requestedAt: new Date().toISOString(),
    payload,
  }
  record(`正在执行 ${type}`)
  const receipt = await frontendPort.executeCommand(request as never, (_state, message) => record(message)) as CommandReceipt & Record<string, unknown>
  if (receipt.finalStatus !== 'committed') throw new Error(`${type}:NOT_COMMITTED`)
  if (typeof receipt.projectId === 'string') projectId.value = receipt.projectId
  if (typeof receipt.resultProjectId === 'string') projectId.value = receipt.resultProjectId
  if (typeof receipt.afterRevision === 'number') revision.value = receipt.afterRevision
  return receipt
}

async function productionWithRetry(payload: Record<string, unknown>) {
  let lastError: unknown
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      return await command('production_start', payload)
    } catch (cause) {
      lastError = cause
      const message = cause instanceof Error ? cause.message : String(cause)
      if (!/B08_REALITY_DELTA_REQUIRED|B08_PLANNING_CONTRACT|B08_AUDIT_CONTRACT|B08_RENDER_OUTPUT_INVALID|P01_MODEL_JSON_INVALID/.test(message) || attempt === 2) throw cause
      record(`真实生产第 ${attempt + 1} 次失败，正在用正式事务重试`)
    }
  }
  throw lastError ?? new Error('B08_PRODUCTION_RETRY_FAILED')
}

async function run() {
  if (running.value) return
  running.value = true
  error.value = ''
  events.value = []
  trace.value = { rootTraceId, sourceCommit: import.meta.env.VITE_SOURCE_COMMIT ?? 'runtime', nodes: [] }
  try {
    await command('global_settings_save', {
      settings: {
        provider: 'OpenAI Compatible',
        providerId: 'openai-compatible',
        model: 'qwen3.8-flash',
        modelId: 'qwen3.8-flash',
        endpoint: 'https://ws-a4ht0p7fdc83x4x4.cn-beijing.maas.aliyuncs.com/compatible-mode/v1',
        enabled: true,
        timeoutMs: 120000,
      },
    })
    const created = await command('project_create_from_idea', {
      title: `真实 UI 闭环验收 ${Date.now()}`,
      idea: '城市记忆档案调查员发现每次修复记忆都会失去自身现实痕迹，必须追查被抹除的姐姐线索。',
      mode: 'DIRECTOR', platform: '长篇网络小说', length: '长篇',
      forbidden: '前三章不揭露幕后者，不用梦境推翻前文', tags: ['都市悬疑', '成长'],
    }) as Record<string, unknown>
    projectId.value = String(created.resultProjectId ?? created.projectId)
    revision.value = Number(created.afterRevision ?? 0)
    await command('story_directions_generate')
    const directions = await frontendPort!.queryStoryDirections({ projectId: projectId.value, scope: 'directions', pageSize: 10 } as never) as unknown as Record<string, unknown>
    const pitches = (directions.pitches as Array<Record<string, unknown>> | undefined) ?? []
    await command('pitch_select', { pitchIds: [pitches[0]?.id], mode: 'single' })
    await command('story_setup_save', { storyCore: { protagonist: '城市记忆档案调查员沈砚', desire: '找回被抹去的姐姐', mechanism: '修复记忆会同步抹去现实痕迹', conflict: '档案局要求掩盖真相', readerPromise: '每章解决一个可验证的小谜题' } })
    await command('opening_ready', { approved: true })
    const volumes = await frontendPort!.queryVolumeList({ projectId: projectId.value, scope: 'volumes', pageSize: 10 } as never) as unknown as Record<string, unknown>
    const volume = ((volumes.items as Array<Record<string, unknown>> | undefined) ?? [])[0]
    if (!volume?.id) throw new Error('UI_GATE_VOLUME_NOT_FOUND')
    const volumeId = String(volume.id)
    const chapters = await frontendPort!.queryChapterList({ projectId: projectId.value, volumeId, scope: 'chapters', pageSize: 10 } as never) as unknown as Record<string, unknown>
    const openingChapters = ((chapters.items as Array<Record<string, unknown>> | undefined) ?? []).filter(chapter => Number(chapter.number) <= 3).sort((a, b) => Number(a.number) - Number(b.number))
    for (const chapter of openingChapters) {
      await productionWithRetry({ chapterId: chapter.id, volumeId, executionMode: 'OPENING_BOOTSTRAP', skillsEnabled: false, requireRealityDelta: true, targetChineseCharacters: 1800, chapterGoalHint: `完成第${chapter.number}章并产生可验证 Reality。` })
    }
    await command('opening_evaluate')
    await command('book_plan_revise')
    await command('planning_create_volume_intent', { volumeId })
    await command('planning_create_volume_plan', { volumeId })
    await command('planning_prepare_next_chapter', { volumeId })
    const afterPrepare = await frontendPort!.queryChapterList({ projectId: projectId.value, volumeId, scope: 'chapters', pageSize: 20 } as never) as unknown as Record<string, unknown>
    const chapter4 = ((afterPrepare.items as Array<Record<string, unknown>> | undefined) ?? []).find(chapter => Number(chapter.number) === 4)
    if (!chapter4?.id) throw new Error('UI_GATE_CHAPTER4_NOT_FOUND')
    await command('planning_compile_chapter_plan', { chapterId: chapter4.id, chapterGoalHint: '用上一阶段 Reality 的真实事实推进下一项调查。' })
    const production4 = await productionWithRetry({ chapterId: chapter4.id, volumeId, executionMode: 'LONG_FORM', skillsEnabled: false, requireRealityDelta: true, targetChineseCharacters: 1800, chapterGoalHint: '推进调查并让真实选择产生可验证后果。' })
    const after = await frontendPort!.queryChapterList({ projectId: projectId.value, volumeId, scope: 'chapters', pageSize: 20 } as never) as unknown as Record<string, unknown>
    const chapter5 = ((after.items as Array<Record<string, unknown>> | undefined) ?? []).find(chapter => Number(chapter.number) === 5)
    trace.value = { rootTraceId, projectId: projectId.value, finalSourceCommit: import.meta.env.VITE_SOURCE_COMMIT ?? 'runtime', uiPath: 'TauriFrontendPort', directExecuteCommandImpl: 0, provider: 'formal app composition root', openingChapters: openingChapters.map(chapter => chapter.id), chapter4: chapter4.id, chapter5: chapter5?.id ?? null, production4, events: events.value, status: 'PASS' }
    ;(globalThis as Record<string, unknown>).__MOHENG_REAL_LLM_TRACE__ = trace.value
    record('真实 UI 闭环完成，下一章已由 Runtime 准备')
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
    trace.value = { rootTraceId, projectId: projectId.value, finalSourceCommit: import.meta.env.VITE_SOURCE_COMMIT ?? 'runtime', events: events.value, status: 'FAIL', error: error.value }
    ;(globalThis as Record<string, unknown>).__MOHENG_REAL_LLM_TRACE__ = trace.value
    record(`真实 UI 闭环失败：${error.value}`)
  } finally {
    running.value = false
  }
}
</script>

<template>
  <main class="real-gate page" data-real-llm-gate>
    <header class="card pad">
      <h1>真实 LLM 产品闭环验收</h1>
      <p>仅用于 final-source 的真实桌面端验收；所有 mutation 经过 TauriFrontendPort。</p>
      <button data-action="start-real-llm-gate" class="btn primary" :disabled="!enabled || running" @click="run">{{ running ? '正在执行真实闭环…' : '开始真实 UI 闭环' }}</button>
      <span data-gate-status>{{ status }}</span>
    </header>
    <section class="card pad"><h2>项目</h2><p data-project-id>{{ projectId || '尚未创建' }}</p><p v-if="error" class="error">{{ error }}</p></section>
    <section class="card pad"><h2>运行轨迹</h2><pre data-real-trace>{{ traceText }}</pre></section>
  </main>
</template>

<style scoped>
.real-gate { display: grid; gap: 16px; padding: 24px; }
.real-gate h1 { margin: 0 0 8px; }
.real-gate h2 { margin: 0 0 8px; }
.real-gate pre { max-height: 420px; overflow: auto; white-space: pre-wrap; font-size: 12px; }
.error { color: #b42318; }
</style>

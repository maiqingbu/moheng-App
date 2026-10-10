<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { productErrorMessage, sanitizeProductText } from '../presentation/productLanguage'
import { formatProgressPercent } from '../presentation/progress'
import { buildReaderRevisionCommands } from '../flows/readerRevision'
import type { ExportCapabilityProjection, ExportProgressProjection, ExportReceiptProjection } from '../../application/contracts'

const store = useProjectStore()
const router = useRouter()
const volumeId = ref('')
const chapterId = ref('')
const query = ref('')
const feedback = ref('')
const revisionError = ref('')
const pendingChangeRequestId = ref('')
const exportCapability = ref<ExportCapabilityProjection | null>(null)
const exportProgress = ref<ExportProgressProjection | null>(null)
const exportReceipt = ref<ExportReceiptProjection | null>(null)
const exportError = ref('')
const exporting = ref(false)
let exportPollTimer: ReturnType<typeof setTimeout> | undefined
let exportGeneration = 0
let exportReaderUnmounted = false
const revisionState = computed(() => store.commandState('reader-revision-proposal'))
const revisionImpactState = computed(() => store.commandState('reader-revision-impact'))
const volumes = computed(() => store.readerVolumes?.items ?? [])
const chapters = computed(() => volumeId.value ? store.chapterLists[volumeId.value]?.items ?? [] : [])
const versionReason = (value: unknown) => sanitizeProductText(value, '这次版本调整已记录。')
const exportProgressRatio = computed(() => {
  const progress = exportProgress.value
  if (!progress || progress.totalCharacters <= 0) return 0
  return Math.min(1, Math.max(0, progress.completedCharacters / progress.totalCharacters))
})
const exportUnavailableMessage = computed(() => {
  if (!exportCapability.value) return '正在核验正式导出能力…'
  if (exportCapability.value.reason === 'DEMO_ONLY') return '演示模式不支持导出正式成品。'
  if (exportCapability.value.reason === 'NO_CURRENT_FINAL_CHAPTERS') return '当前作品还没有可导出的正式章节。'
  return '当前暂不可导出正式成品。'
})

function exportErrorMessage(error: unknown) {
  const code = error instanceof Error ? error.message : String(error)
  const messages: Record<string, string> = {
    EXPORT_UNAVAILABLE_IN_BROWSER_DEMO: '浏览器演示模式不能导出正式成品。',
    EXPORT_IO_ERROR: '文件写入失败，未生成正式导出回执。请检查磁盘空间后重试。',
    EXPORT_SECURITY_SCAN_FAILED: '导出内容触发安全检查，成品和回执均未生成。请检查正文中的密钥或内部资料。',
    EXPORT_PATH_INVALID: '导出目录不可用，请检查作品数据目录后重试。',
    EXPORT_SOURCE_READ_FAILED: '读取正式章节失败，未生成导出成品。',
    EXPORT_SOURCE_CHANGED: '正式章节在导出时发生变化，请刷新后重试。',
    EXPORT_SOURCE_NUL_UNSUPPORTED: '正式章节包含当前 TXT 格式无法安全处理的空字符，未生成成品或回执。',
    EXPORT_PROJECT_MISMATCH: '导出回执与当前作品或请求不匹配，未显示该回执。',
    EXPORT_ALREADY_RUNNING: '这项导出仍在处理中，请稍后查看进度。',
    EXPORT_PROJECT_NOT_READY: '作品当前不可导出，请完成恢复或维护后重试。',
    EXPORT_FINALIZE_FAILED: '导出文件未能正式完成，请重试。',
    NO_CURRENT_FINAL_CHAPTERS: '当前作品还没有可导出的正式章节。',
  }
  return messages[code] ?? productErrorMessage(error, '导出没有完成，正式回执尚未生成。')
}

function clearExportPoll() {
  if (exportPollTimer) clearTimeout(exportPollTimer)
  exportPollTimer = undefined
}

function isCurrentExport(projectId: string, generation: number) {
  return !exportReaderUnmounted && store.currentProjectId === projectId && exportGeneration === generation
}

async function refreshExportCapability(projectId = store.currentProjectId, generation = exportGeneration) {
  if (!projectId) return
  exportError.value = ''
  try {
    const capability = await store.loadExportCapability(projectId)
    if (!isCurrentExport(projectId, generation)) return
    exportCapability.value = capability
    const latest = await store.loadLatestExportReceipt(projectId)
    if (!isCurrentExport(projectId, generation)) return
    if (latest.status === 'COMPLETED' && latest.projectId !== projectId) {
      throw new Error('EXPORT_PROJECT_MISMATCH')
    }
    exportReceipt.value = latest.status === 'COMPLETED' ? latest : null
  } catch (error) {
    if (isCurrentExport(projectId, generation)) exportError.value = exportErrorMessage(error)
  }
}

async function pollExport(requestId: string, projectId: string, generation: number) {
  if (!exporting.value || !isCurrentExport(projectId, generation)) return
  try {
    const status = await store.queryExportReceipt(requestId, projectId)
    if (!isCurrentExport(projectId, generation)) return
    if (status.status !== 'NONE' && status.requestId !== requestId) {
      throw new Error('EXPORT_PROJECT_MISMATCH')
    }
    if (status.status === 'COMPLETED' && status.projectId !== projectId) {
      throw new Error('EXPORT_PROJECT_MISMATCH')
    }
    if (status.status === 'RUNNING' || status.status === 'FINALIZING') exportProgress.value = status
    if (status.status === 'COMPLETED') {
      exportReceipt.value = status
      exportProgress.value = null
      exporting.value = false
    }
  } catch {
    // The create command may not have published its first progress snapshot yet.
  }
  if (exporting.value && isCurrentExport(projectId, generation)) {
    exportPollTimer = setTimeout(() => void pollExport(requestId, projectId, generation), 450)
  }
}

async function exportFinal() {
  if (!exportCapability.value?.available || exporting.value) return
  const projectId = store.currentProjectId
  if (!projectId) return
  const generation = ++exportGeneration
  const isCurrent = () => isCurrentExport(projectId, generation)
  exporting.value = true
  exportError.value = ''
  exportReceipt.value = null
  const requestId = crypto.randomUUID()
  exportPollTimer = setTimeout(() => void pollExport(requestId, projectId, generation), 150)
  try {
    const receipt = await store.createExport(requestId, 'TXT', projectId)
    if (!isCurrent()) return
    if (receipt.projectId !== projectId || receipt.requestId !== requestId) {
      throw new Error('EXPORT_PROJECT_MISMATCH')
    }
    if (receipt.status !== 'COMPLETED') throw new Error('EXPORT_RECEIPT_MISSING')
    exportReceipt.value = receipt
    exportProgress.value = null
  } catch (error) {
    if (!isCurrent()) return
    if (exportReceipt.value?.status === 'COMPLETED') return
    try {
      const recovered = await store.queryExportReceipt(requestId, projectId)
      if (!isCurrent()) return
      if (recovered.status !== 'NONE'
        && (recovered.requestId !== requestId || ('projectId' in recovered && recovered.projectId !== projectId))) {
        throw new Error('EXPORT_PROJECT_MISMATCH')
      }
      if (recovered.status === 'COMPLETED') {
        exportReceipt.value = recovered
        exportProgress.value = null
        return
      }
    } catch {
      // Report the original typed failure; query errors never imply success.
    }
    exportError.value = exportErrorMessage(error)
  } finally {
    if (isCurrent()) {
      exporting.value = false
      clearExportPoll()
    }
  }
}

watch(() => store.currentProjectId, projectId => {
  exportGeneration += 1
  clearExportPoll()
  exporting.value = false
  exportCapability.value = null
  exportProgress.value = null
  exportReceipt.value = null
  exportError.value = ''
  if (projectId) void refreshExportCapability(projectId, exportGeneration)
}, { flush: 'sync' })

async function pickVolume(id: string) {
  volumeId.value = id
  await store.loadChapterList(id)
  if (chapters.value[0]) await pickChapter(chapters.value[0].id)
}
async function pickChapter(id: string) {
  chapterId.value = id
  await Promise.all([store.loadChapter(id), store.loadChapterVersions(id)])
}
async function openHit(hit: { volumeId: string; chapterId: string }) {
  volumeId.value = hit.volumeId
  await store.loadChapterList(hit.volumeId)
  await pickChapter(hit.chapterId)
}
async function search() {
  if (!query.value.trim()) return
  await store.search(query.value.trim())
}
async function revise() {
  const projectId = store.currentProjectId
  const chapter = store.activeChapter
  if (!projectId || !chapter || chapter.id !== chapterId.value || !feedback.value.trim()) return
  revisionError.value = ''
  pendingChangeRequestId.value = ''
  try {
    const commands = buildReaderRevisionCommands({
      projectId,
      chapterId: chapter.id,
      chapterNumber: chapter.number,
      chapterTitle: chapter.title,
      feedback: feedback.value,
    })
    const created = await store.runCommand('reader-revision-proposal', commands.proposal)
    if (created.finalStatus !== 'committed') throw new Error(created.message || '修改请求未能保存。')
    pendingChangeRequestId.value = commands.changeRequestId
    const preflight = await store.runCommand('reader-revision-impact', commands.impact)
    const bootstrap = preflight.result?.status === 'SUMMARY_BOOTSTRAP_REQUIRED' ? '1' : undefined
    feedback.value = ''
    await router.push({
      name: 'history',
      params: { id: projectId },
      query: { changeRequestId: commands.changeRequestId, ...(bootstrap ? { bootstrap } : {}) },
    })
  } catch (error) {
    revisionError.value = pendingChangeRequestId.value
      ? '修改请求已保存，但影响分析未完成。你可以到版本与变更记录继续核对。'
      : productErrorMessage(error, '修改请求未能保存。')
  }
}

async function openPendingChangeRequest() {
  if (!store.currentProjectId || !pendingChangeRequestId.value) return
  await router.push({
    name: 'history',
    params: { id: store.currentProjectId },
    query: { changeRequestId: pendingChangeRequestId.value },
  })
}

onMounted(async () => {
  await store.loadReaderVolumes()
  await refreshExportCapability(store.currentProjectId, exportGeneration)
  if (volumes.value[0]) await pickVolume(volumes.value[0].id)
})

onUnmounted(() => {
  exportReaderUnmounted = true
  exportGeneration += 1
  clearExportPoll()
})
</script>

<template>
  <div class="page">
    <PageHeader title="资料 · 成品阅读" subtitle="按卷打开正式章节，也可以从人物、事件、线索与设定角度查看作品记录。">
      <div class="actions"><input v-model="query" class="input" style="width:220px" placeholder="搜索正文 / 人名 / 地点" @keyup.enter="search"><button class="btn" @click="search">搜索</button></div>
    </PageHeader>

    <div v-if="store.searchResults.items.length" class="card pad" style="margin-bottom:14px">
      <strong>搜索结果</strong>
      <button v-for="hit in store.searchResults.items" :key="hit.id" class="chapter-link" @click="openHit(hit)">第 {{ hit.chapterNumber }} 章 · {{ hit.title }} — {{ hit.excerpt }}</button>
    </div>

    <div class="reader-layout">
      <aside class="chapter-nav card pad">
        <select v-model="volumeId" class="select" @change="pickVolume(volumeId)"><option v-for="volume in volumes" :key="volume.id" :value="volume.id">第 {{ volume.number }} 卷 · {{ volume.title }}</option></select>
        <button v-for="chapter in chapters" :key="chapter.id" class="chapter-link" :class="{ active: chapter.id === chapterId, producing: chapter.status === 'producing' }" @click="pickChapter(chapter.id)">第 {{ chapter.number }} 章 · {{ chapter.title }}</button>
      </aside>

      <article v-if="store.activeChapter" class="manuscript">
        <h1>第 {{ store.activeChapter.number }} 章 {{ store.activeChapter.title }}</h1>
        <div class="manuscript-meta">{{ store.activeChapter.wordCount.toLocaleString() }} 字 · {{ store.activeChapter.version }}</div>
        <p v-for="(paragraph, index) in store.activeChapter.content" :key="index">{{ paragraph }}</p>
      </article>

      <aside class="reader-tools">
        <section class="card pad">
          <h2 class="card-title">版本</h2>
          <div v-for="version in store.chapterVersions.items" :key="version.id" class="version-row"><strong>{{ version.label }}</strong><div class="muted" style="font-size:11px">{{ versionReason(version.reason) }}</div></div>
        </section>

        <section class="card pad">
          <h2 class="card-title">导出正式成品</h2>
          <p class="muted">仅导出已提交并仍处于当前正式版本的章节；格式：TXT。</p>
          <p v-if="!exportCapability?.available" class="muted" aria-live="polite">
            {{ exportUnavailableMessage }}
          </p>
          <div v-if="exportProgress" class="export-progress" aria-live="polite">
            <span>{{ exportProgress.status === 'FINALIZING' ? '正在完成文件并写入回执' : `正在导出第 ${exportProgress.currentChapterNumber ?? '—'} 章` }}（{{ formatProgressPercent(exportProgressRatio) }}）</span>
            <progress :value="exportProgressRatio" max="1" aria-label="正式成品导出进度" />
          </div>
          <p v-if="exportError" class="banner warning" role="alert">{{ exportError }}</p>
          <div v-if="exportReceipt?.status === 'COMPLETED'" class="banner success" aria-live="polite">
            <strong>正式成品已生成：{{ exportReceipt.fileName }}</strong>
            <div>{{ exportReceipt.chapters.length }} 章 · {{ exportReceipt.sourceCharacterCount.toLocaleString() }} 字 · SHA-256 {{ exportReceipt.sha256 }}</div>
            <div class="export-path">{{ exportReceipt.finalPath }}</div>
          </div>
          <button class="btn" style="width:100%;margin-top:8px" :disabled="!exportCapability?.available || exporting" @click="exportFinal">
            {{ exporting ? '正在导出…' : '导出正式成品 TXT' }}
          </button>
        </section>

        <section class="card pad">
          <h2 class="card-title">不满意这章？</h2>
          <textarea v-model="feedback" class="textarea" placeholder="例如：节奏更快，主角更主动" />
          <CommandFeedback :state="revisionState" />
          <CommandFeedback :state="revisionImpactState" />
          <p v-if="revisionError" class="banner warning">{{ revisionError }}</p>
          <button v-if="pendingChangeRequestId" class="btn" style="margin-top:8px" @click="openPendingChangeRequest">查看已保存的修改请求</button>
          <button class="btn primary" style="margin-top:8px" :disabled="!chapterId || !feedback.trim()" @click="revise">创建修订请求</button>
          <div class="muted" style="font-size:10px;margin-top:7px">请求会进入版本与变更记录并先核对影响，不会直接覆盖正式章节。</div>
        </section>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.export-progress { display: grid; gap: 7px; margin: 10px 0; font-size: 12px; }
.export-progress progress { width: 100%; }
.export-path { margin-top: 5px; font-size: 11px; overflow-wrap: anywhere; }
</style>

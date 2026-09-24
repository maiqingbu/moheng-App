<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { productErrorMessage, sanitizeProductText } from '../presentation/productLanguage'
import { buildReaderRevisionCommands } from '../flows/readerRevision'

const store = useProjectStore()
const router = useRouter()
const volumeId = ref('')
const chapterId = ref('')
const query = ref('')
const feedback = ref('')
const revisionError = ref('')
const pendingChangeRequestId = ref('')
const exportCapabilityAvailable = false
const revisionState = computed(() => store.commandState('reader-revision-proposal'))
const revisionImpactState = computed(() => store.commandState('reader-revision-impact'))
const volumes = computed(() => store.readerVolumes?.items ?? [])
const chapters = computed(() => volumeId.value ? store.chapterLists[volumeId.value]?.items ?? [] : [])
const versionReason = (value: unknown) => sanitizeProductText(value, '这次版本调整已记录。')

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
  if (volumes.value[0]) await pickVolume(volumes.value[0].id)
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
          <p class="muted">正式导出功能尚未开放，当前不能创建导出任务。</p>
          <button class="btn" style="width:100%;margin-top:8px" :disabled="!exportCapabilityAvailable">导出暂不可用</button>
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

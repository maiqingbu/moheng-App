<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { sanitizeProductText } from '../presentation/productLanguage'

const store = useProjectStore()
const volumeId = ref('')
const chapterId = ref('')
const query = ref('')
const feedback = ref('')
const exportScope = ref<'chapter' | 'range' | 'volume' | 'book'>('chapter')
const rangeStart = ref(1)
const rangeEnd = ref(3)
const revisionState = computed(() => store.commandState('reader-revision'))
const exportState = computed(() => store.commandState('reader-export'))
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
  if (!chapterId.value || !feedback.value.trim()) return
  await store.runCommand('reader-revision', {
    type: 'chapter_revision_request',
    payload: { chapterId: chapterId.value, feedback: feedback.value.trim() }
  })
  feedback.value = ''
}
async function exportFinal() {
  const payload: Record<string, unknown> = { scope: exportScope.value, finalOnly: true }
  if (exportScope.value === 'chapter') payload.chapterId = chapterId.value
  if (exportScope.value === 'volume') payload.volumeId = volumeId.value
  if (exportScope.value === 'range') {
    payload.chapterStart = rangeStart.value
    payload.chapterEnd = rangeEnd.value
  }
  await store.runCommand('reader-export', { type: 'export_create', payload })
}

onMounted(async () => {
  await store.loadReaderVolumes()
  if (volumes.value[0]) await pickVolume(volumes.value[0].id)
})
</script>

<template>
  <div class="page">
    <PageHeader title="成品阅读" subtitle="按卷打开目录，正文只读取当前章节；整书导出会在后台完成。">
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
          <select v-model="exportScope" class="select">
            <option value="chapter">当前章节</option><option value="range">章节范围</option><option value="volume">当前卷</option><option value="book">整书</option>
          </select>
          <div v-if="exportScope === 'range'" class="setting-grid" style="margin-top:8px"><label class="field"><span class="field-label">起始章</span><input v-model.number="rangeStart" class="input" type="number" min="1"></label><label class="field"><span class="field-label">结束章</span><input v-model.number="rangeEnd" class="input" type="number" :min="rangeStart"></label></div>
          <CommandFeedback :state="exportState" />
          <button class="btn" style="width:100%;margin-top:8px" @click="exportFinal">创建导出任务</button>
          <div class="muted" style="font-size:10px;margin-top:7px">格式使用项目设置中的默认导出格式；只导出正式提交版本。</div>
        </section>

        <section class="card pad">
          <h2 class="card-title">不满意这章？</h2>
          <textarea v-model="feedback" class="textarea" placeholder="例如：节奏更快，主角更主动" />
          <CommandFeedback :state="revisionState" />
          <button class="btn primary" style="margin-top:8px" :disabled="!feedback.trim()" @click="revise">创建修订请求</button>
        </section>
      </aside>
    </div>
  </div>
</template>

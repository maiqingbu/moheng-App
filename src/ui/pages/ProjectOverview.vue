<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { blockReasonLabels, phaseLabel, signingLabels } from '../presentation/projectLabels'

const store = useProjectStore()
const router = useRouter()
const project = computed(() => store.project)
const overview = computed(() => store.overview)
const id = computed(() => store.currentProjectId ?? '')
const currentVolume = computed(() => overview.value?.currentVolume)
const recentChapters = computed(() => overview.value?.recentChapters ?? [])
const pending = computed(() => store.pendingDecisionCount ?? 0)
const nextChapter = computed(() => recentChapters.value.length ? recentChapters.value[0].number + 1 : 1)

// 2026-11-27: one next step, said plainly, from the work's own state; the
// button goes exactly where that step is done.
const nextStep = computed(() => {
  const state = project.value?.runState
  const phase = project.value?.phase
  if (pending.value) return {
    title: `先处理 ${pending.value} 件待决定`,
    note: '它们会影响接下来怎么写，处理完就能继续。',
    action: '去处理', to: `/projects/${id.value}/decisions`,
  }
  if (state === 'ACTIVE') return {
    title: '正在写作中',
    note: '写完、检查完会自动入库，可以随时去看进度。',
    action: '查看写作进度', to: `/projects/${id.value}/production`,
  }
  if (state === 'BLOCKED' || state === 'FAILED') return {
    title: '写作停下来了',
    note: project.value?.blockReason ? `原因：${blockReasonLabels[project.value.blockReason]}。` : '上一次没有写完，需要看一下原因。',
    action: '去看原因', to: `/projects/${id.value}/production`,
  }
  if (state === 'PAUSED') return {
    title: '写作已暂停',
    note: '准备好了就从写作页继续。',
    action: '去写作页', to: `/projects/${id.value}/production`,
  }
  if (phase === 'PITCHING' || phase === 'STORY_BLUEPRINT') return {
    title: '先定好故事方向',
    note: '方向和设定确认后，就能开始写第一章。',
    action: '去定方向', to: `/projects/${id.value}/directions`,
  }
  return {
    title: `写第 ${nextChapter.value} 章`,
    note: recentChapters.value.length ? `第 ${recentChapters.value[0].number} 章已经入库。` : '设定已经就绪，可以开始写第一章。',
    action: '去写作', to: `/projects/${id.value}/production`,
  }
})

const stageMetric = computed(() => currentVolume.value
  ? { value: `第 ${currentVolume.value.number} 卷`, note: currentVolume.value.title }
  : { value: phaseLabel(project.value?.phase), note: '还没有进入分卷写作' })
const signingMetric = computed(() => project.value ? signingLabels[project.value.signingStatus] : '未评估')
const cost = computed(() => (project.value?.totalCost ?? 0).toFixed(2))

const healthClass = computed(() => {
  switch (overview.value?.healthSummary.level) {
    case 'risk': return 'danger'
    case 'attention': return 'warn'
    case 'healthy': return 'brand'
    default: return 'muted'
  }
})
const healthBadge = computed(() => {
  switch (overview.value?.healthSummary.level) {
    case 'risk': return '需处理'
    case 'attention': return '关注'
    case 'healthy': return '稳定'
    case 'source_unavailable': return '暂不可用'
    case 'insufficient_data': return '数据不足'
    case 'not_evaluated': return '未评估'
    default: return '暂不可用'
  }
})
const healthTitle = computed(() => overview.value?.healthSummary.label ?? '正在积累作品记录')
const healthNote = computed(() => overview.value?.healthSummary.note ?? '正式章节与项目记录积累后，会显示长期状态。')
</script>

<template>
  <div v-if="project && overview" class="prototype-overview">
    <div class="prototype-pagehead"><div><h2>{{ project.title }}</h2><p>{{ [project.subtitle, project.platform, project.genre].filter(Boolean).join(' · ') }}</p></div></div>

    <section class="home-next" aria-labelledby="home-next-title">
      <div class="home-next-copy">
        <span class="home-next-label">下一步</span>
        <strong id="home-next-title">{{ nextStep.title }}</strong>
        <span>{{ nextStep.note }}</span>
      </div>
      <button class="prototype-btn home-next-action" type="button" @click="router.push(nextStep.to)">{{ nextStep.action }}</button>
    </section>

    <div class="prototype-grid3 home-metrics">
      <div class="prototype-panel"><div class="prototype-metric-label">已写</div><div class="prototype-metric">{{ project.committedChapters }} 章</div></div>
      <div class="prototype-panel"><div class="prototype-metric-label">当前阶段</div><div class="prototype-metric">{{ stageMetric.value }}</div><small class="muted">{{ stageMetric.note }}</small></div>
      <div class="prototype-panel"><div class="prototype-metric-label">开篇签约评估</div><div class="prototype-metric">{{ signingMetric }}</div></div>
      <div class="prototype-panel"><div class="prototype-metric-label">已花费</div><div class="prototype-metric">¥{{ cost }}</div></div>
    </div>

    <div class="prototype-grid" style="margin-top:16px">
      <section class="prototype-panel">
        <h3>最近章节</h3>
        <div v-if="recentChapters.length" class="prototype-list"><div v-for="chapter in recentChapters.slice(0, 3)" :key="chapter.id" class="prototype-item"><div><strong>第 {{ chapter.number }} 章 · {{ chapter.title }}</strong><small>已入库 · {{ chapter.wordCount.toLocaleString() }} 字</small></div></div></div>
        <div v-else class="prototype-item"><div><strong>还没有入库的章节</strong><small>第一章写完并通过检查后，会显示在这里。</small></div></div>
        <button v-if="recentChapters.length" class="prototype-btn" type="button" style="margin-top:10px" @click="router.push(`/projects/${id}/read`)">打开稿件</button>
      </section>

      <aside class="prototype-panel">
        <h3>待决定</h3>
        <div v-if="pending" class="prototype-item"><div><strong>{{ pending }} 件事等你决定</strong><small>系统不会替你拿主意，处理前相关的写作会等着。</small></div><button class="prototype-btn" type="button" @click="router.push(`/projects/${id}/decisions`)">去处理</button></div>
        <div v-else class="prototype-item"><div><strong>没有要你决定的事</strong><small>需要你确认、修改或授权的事会出现在这里。</small></div></div>
        <div style="height:16px" />
        <h3>作品状况</h3>
        <div class="prototype-item"><div><strong>{{ healthTitle }}</strong><small>{{ healthNote }}</small></div><span class="prototype-badge" :class="healthClass">{{ healthBadge }}</span><button class="prototype-btn" type="button" @click="router.push(`/projects/${id}/health`)">详情</button></div>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.home-next { margin-top: 16px; display: flex; flex-wrap: wrap; align-items: center; gap: 20px; padding: 24px 28px; border-radius: var(--radius-lg); background: var(--pine-800); color: #ffffff; }
.home-next-copy { flex: 1 1 320px; display: flex; flex-direction: column; gap: 4px; }
.home-next-copy strong { font-size: 22px; line-height: 1.4; }
.home-next-copy span { color: var(--pine-100); }
.home-next-label { font-size: 13px; }
.home-next-action { background: #ffffff; color: var(--pine-800); border-color: #ffffff; font-weight: 600; padding: 10px 20px; }
.home-metrics { margin-top: 16px; grid-template-columns: repeat(4, minmax(0, 1fr)); }
@media (max-width: 900px) { .home-metrics { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>

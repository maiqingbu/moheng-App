<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { phaseLabel } from '../presentation/projectLabels'

const store = useProjectStore()
const router = useRouter()
const project = computed(() => store.project)
const overview = computed(() => store.overview)
const id = computed(() => store.currentProjectId ?? '')
const currentVolume = computed(() => overview.value?.currentVolume)
const recentChapters = computed(() => overview.value?.recentChapters ?? [])
const nextAction = computed(() => store.pendingDecisionCount
  ? { label: `处理 ${store.pendingDecisionCount} 项待处理`, to: `/projects/${id.value}/decisions` }
  : project.value?.phase === 'PITCHING' || project.value?.phase === 'STORY_BLUEPRINT'
    ? { label: '进入规划', to: `/projects/${id.value}/directions` }
    : { label: '进入创作', to: `/projects/${id.value}/production` })
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
const nextChapterLabel = computed(() => recentChapters.value.length ? `继续第 ${recentChapters.value[0].number + 1} 章创作` : '开始第一章创作')
</script>

<template>
  <div v-if="project && overview" class="prototype-overview">
    <div class="prototype-pagehead"><div><h2>总览</h2><p>继续写作、掌握当前卷状态，并优先处理真正会影响后续创作的问题。</p></div><button class="prototype-btn" type="button" @click="router.push(`/projects/${id}/assistant`)">页面帮助</button></div>

    <div v-if="project.runState === 'IDLE'" class="prototype-notice"><strong>等待开始</strong><span>项目已经准备好，尚未启动新的生产任务。</span></div>

    <div class="prototype-grid3" :style="project.runState === 'IDLE' ? 'margin-top:16px' : ''">
      <div class="prototype-panel"><div class="prototype-metric">{{ project.committedChapters }}</div><div class="prototype-metric-label">已完成章节</div></div>
      <div class="prototype-panel"><div class="prototype-metric">{{ currentVolume ? `第 ${currentVolume.number} 卷` : phaseLabel(project.phase) }}</div><div class="prototype-metric-label">当前创作阶段</div></div>
      <div class="prototype-panel"><div class="prototype-metric warning">{{ store.pendingDecisionCount }}</div><div class="prototype-metric-label">需要优先处理</div></div>
    </div>

    <div class="prototype-grid" style="margin-top:16px">
      <section class="prototype-panel">
        <h3>继续创作</h3>
        <div class="prototype-item"><div><strong>{{ nextChapterLabel }}</strong><small>{{ recentChapters.length ? '最近正式章节已经确认，可以继续准备下一章。' : '先完成故事方向与第一章准备，再开始正式创作。' }}</small></div><button class="prototype-badge brand" type="button" @click="router.push(nextAction.to)">{{ nextAction.label }}</button></div>
        <div style="height:10px" />
        <h3>最近章节</h3>
        <div v-if="recentChapters.length" class="prototype-list"><div v-for="chapter in recentChapters.slice(0, 3)" :key="chapter.id" class="prototype-item"><div><strong>第 {{ chapter.number }} 章 · {{ chapter.title }}</strong><small>已确认 · {{ chapter.wordCount.toLocaleString() }} 字</small></div></div></div>
        <div v-else class="prototype-item"><div><strong>还没有正式章节</strong><small>第一章确认后，最近章节会显示在这里。</small></div></div>
      </section>

      <aside class="prototype-panel">
        <h3>需要处理</h3>
        <div v-if="store.pendingDecisionCount" class="prototype-list"><div class="prototype-item"><div><strong>有 {{ store.pendingDecisionCount }} 项内容需要确认</strong><small>它们会在确认后影响下一步创作安排。</small></div><span class="prototype-badge danger">需要处理</span></div></div>
        <div v-else class="prototype-list"><div class="prototype-item"><div><strong>当前没有待确认事项</strong><small>需要确认、修改或重新检查的内容会集中显示在这里。</small></div><span class="prototype-badge brand">正常</span></div></div>
        <div style="height:16px" />
        <h3>长篇状态</h3>
        <div class="prototype-item"><div><strong>{{ healthTitle }}</strong><small>{{ healthNote }}</small></div><span class="prototype-badge" :class="healthClass">{{ healthBadge }}</span></div>
      </aside>
    </div>
  </div>
</template>

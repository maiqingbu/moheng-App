<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'
import ProjectAssistantDock from './ProjectAssistantDock.vue'
import WriteLockBanner from './WriteLockBanner.vue'
import LoadingBlock from './LoadingBlock.vue'
import { phaseLabel } from '../presentation/projectLabels'
import { productErrorMessage } from '../presentation/productLanguage'

const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const library = useProjectLibraryStore()
const assistantCollapsed = ref(false)
const workspaceEl = ref<HTMLElement | null>(null)
const id = computed(() => store.currentProjectId ?? (typeof route.params.id === 'string' ? route.params.id : ''))
const inWorkspace = computed(() => route.meta.scope === 'project' && !!id.value)
const projectTitle = computed(() => store.project?.title?.trim() || (store.loading ? '正在读取…' : '未命名作品'))
const phase = computed(() => store.project ? phaseLabel(store.project.phase) : '作品库')
const currentVolume = computed(() => store.overview?.currentVolume ? `第 ${store.overview.currentVolume.number} 卷 · ${store.overview.currentVolume.title}` : phase.value)
const committedChapters = computed(() => store.project?.committedChapters ?? 0)

type NavItem = { label: string; icon: string; to: string; matches: string[] }
// 2026-11-27 (owner approved the redesign): five task areas. Decisions are
// in the top bar and on the home page; every former page keeps its route.
const workspaceNav = computed<NavItem[]>(() => inWorkspace.value ? [
  { label: '首页', icon: '⌂', to: `/projects/${id.value}`, matches: ['project-overview', 'decisions'] },
  { label: '写作', icon: '✎', to: `/projects/${id.value}/production`, matches: ['production', 'opening-ready'] },
  // A work with a volume opens on its route; one still being set up on its direction.
  { label: '故事', icon: '◇', to: store.overview?.currentVolume ? `/projects/${id.value}/volumes` : `/projects/${id.value}/directions`, matches: ['directions', 'market', 'story-setup', 'craft-setup', 'volumes', 'volume-workspace', 'completion'] },
  // 版本与变更记录 (match: 'history') belongs with the manuscript.
  { label: '稿件', icon: '▤', to: `/projects/${id.value}/read`, matches: ['read', 'history'] },
  // 长篇健康 (/health) belongs with the work's records.
  { label: '资料库', icon: '◎', to: `/projects/${id.value}/graph`, matches: ['graph', 'health'] },
] : [])
const isActive = (item: NavItem) => item.matches.includes(String(route.name))
const pendingDecisions = computed(() => store.pendingDecisionCount ?? 0)
// Below this width the assistant floats over the page, so it starts folded
// (the same 1180px breakpoint as prototype-shell.css).
const narrowWindow = typeof window !== 'undefined' && typeof window.matchMedia === 'function'
  ? window.matchMedia('(max-width: 1180px)')
  : null
watch(inWorkspace, value => { if (value) assistantCollapsed.value = narrowWindow?.matches ?? false }, { immediate: true })
const foldOnNarrow = (event: MediaQueryListEvent) => { if (event.matches) assistantCollapsed.value = true }
narrowWindow?.addEventListener('change', foldOnNarrow)
onUnmounted(() => narrowWindow?.removeEventListener('change', foldOnNarrow))
watch(() => route.fullPath, () => {
  void nextTick(() => {
    if (workspaceEl.value) workspaceEl.value.scrollTop = 0
  })
}, { flush: 'post', immediate: true })
</script>

<template>
  <div class="prototype-window">
    <div class="prototype-topbar">
      <button class="prototype-brand" type="button" @click="router.push('/projects')"><span class="prototype-logo">墨</span><span class="prototype-brand-copy"><strong>墨衡</strong><small>长篇小说创作工作台</small></span></button>
      <div class="prototype-projectbar"><button v-if="inWorkspace" class="prototype-btn ghost" type="button" @click="router.push('/projects')"><strong>{{ projectTitle }}</strong></button><strong v-else>作品库</strong><span v-if="inWorkspace" class="prototype-pill">{{ currentVolume }}</span><span v-if="inWorkspace" class="prototype-projectbar-meta">已写 {{ committedChapters }} 章</span></div>
      <div class="prototype-topactions"><RouterLink v-if="inWorkspace && pendingDecisions" :to="`/projects/${id}/decisions`" class="prototype-btn decision-pill">{{ pendingDecisions }} 件事等你决定</RouterLink><button v-if="!inWorkspace" class="prototype-btn primary" type="button" @click="router.push('/projects/new')">＋ 新建作品</button><button v-if="inWorkspace" class="prototype-iconbtn" type="button" :aria-label="assistantCollapsed ? '展开项目助手' : '折叠项目助手'" @click="assistantCollapsed = !assistantCollapsed">{{ assistantCollapsed ? '‹' : '›' }}</button><button class="prototype-iconbtn" type="button" aria-label="作品设置" @click="router.push(inWorkspace ? `/projects/${id}/settings` : '/settings')">⚙</button></div>
    </div>

    <div v-if="store.activity" class="shell-activity" :class="store.activity" role="status" aria-live="polite"><div class="progress-running" /><span v-if="store.activity === 'command'" class="shell-activity-label"><span class="spinner" />正在处理…</span></div>
    <div class="prototype-shell" :class="{ 'assistant-collapsed': assistantCollapsed, 'no-assistant': !inWorkspace }">
      <nav class="prototype-nav" aria-label="作品导航">
        <template v-if="inWorkspace">
          <RouterLink v-for="item in workspaceNav" :key="item.label" :to="item.to" class="prototype-navbutton" :class="{ active: isActive(item) }" :aria-current="isActive(item) ? 'page' : undefined"><span class="ico" aria-hidden="true">{{ item.icon }}</span><span>{{ item.label }}</span></RouterLink>
          <div class="prototype-navspacer" />
          <RouterLink :to="`/projects/${id}/settings`" class="prototype-navbutton" :class="{ active: route.name === 'project-settings' }"><span class="ico">⚙</span><span>作品设置</span></RouterLink>
        </template>
        <template v-else>
          <div class="prototype-nav-section">作品</div>
          <RouterLink to="/projects" class="prototype-navbutton" :class="{ active: route.name === 'projects' }"><span class="ico">▦</span><span>作品库</span><span v-if="library.projection" class="prototype-navcount">{{ library.projection.totals.active }}</span></RouterLink>
          <RouterLink to="/projects/new" class="prototype-navbutton" :class="{ active: route.name === 'new-project' }"><span class="ico">＋</span><span>新建作品</span></RouterLink>
          <div class="prototype-navspacer" />
          <RouterLink to="/settings" class="prototype-navbutton" :class="{ active: route.name === 'settings' }"><span class="ico">⚙</span><span>应用设置</span></RouterLink>
        </template>
      </nav>

      <main ref="workspaceEl" class="prototype-workspace"><WriteLockBanner :locked="inWorkspace && store.writeLocked" /><div v-if="inWorkspace && store.loadError" class="prototype-panel"><strong>暂时无法读取作品</strong><p>{{ productErrorMessage(store.loadError, '请返回作品库后重试。') }}</p><button class="prototype-btn" type="button" @click="store.refresh">重试</button></div><LoadingBlock v-else-if="inWorkspace && store.loading && !store.overview" class="prototype-panel" caption="正在读取作品记录…" /><slot v-else /></main>
      <ProjectAssistantDock v-if="inWorkspace" :collapsed="assistantCollapsed" @toggle="assistantCollapsed = !assistantCollapsed" />
    </div>
  </div>
</template>

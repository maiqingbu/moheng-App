<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'
import ProjectAssistantDock from './ProjectAssistantDock.vue'
import WriteLockBanner from './WriteLockBanner.vue'
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
const projectTitle = computed(() => store.project?.title?.trim() || '未命名作品')
const phase = computed(() => store.project ? phaseLabel(store.project.phase) : '作品库')
const currentVolume = computed(() => store.overview?.currentVolume ? `第 ${store.overview.currentVolume.number} 卷 · ${store.overview.currentVolume.title}` : phase.value)
const committedChapters = computed(() => store.project?.committedChapters ?? 0)

type NavItem = { label: string; icon: string; to: string; matches: string[]; count?: number; section?: string }
const workspaceNav = computed<NavItem[]>(() => inWorkspace.value ? [
  { label: '总览', icon: '⌂', to: `/projects/${id.value}`, matches: ['project-overview'], section: '作品' },
  { label: '创作', icon: '✎', to: `/projects/${id.value}/production`, matches: ['production'] },
  { label: '规划', icon: '◇', to: `/projects/${id.value}/directions`, matches: ['directions', 'market', 'story-setup', 'opening-ready'] },
  { label: '当前卷', icon: '▤', to: `/projects/${id.value}/volumes`, matches: ['volumes', 'volume-workspace'] },
  { label: '待处理', icon: '✓', to: `/projects/${id.value}/decisions`, matches: ['decisions', 'history'], count: store.pendingDecisionCount, section: '管理' },
  { label: '长篇健康', icon: '♡', to: `/projects/${id.value}/health`, matches: ['health'] },
  { label: '资料', icon: '⌘', to: `/projects/${id.value}/read`, matches: ['read', 'graph'] },
  { label: '完结准备', icon: '◯', to: `/projects/${id.value}/completion`, matches: ['completion'] },
] : [])
const isActive = (item: NavItem) => item.matches.includes(String(route.name))
// 冻结路由兼容：长篇健康继续使用 /health；版本与变更记录保留 match: 'history'，但统一归入待处理。
watch(inWorkspace, value => { if (value) assistantCollapsed.value = false }, { immediate: true })
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
      <div class="prototype-topactions"><button class="prototype-btn ghost" type="button" @click="router.push('/projects')">⌘K 搜索</button><button class="prototype-btn primary" type="button" @click="router.push('/projects/new')">＋ 新建</button><button v-if="inWorkspace" class="prototype-iconbtn" type="button" :aria-label="assistantCollapsed ? '展开项目助手' : '折叠项目助手'" @click="assistantCollapsed = !assistantCollapsed">{{ assistantCollapsed ? '‹' : '›' }}</button><button class="prototype-iconbtn" type="button" aria-label="作品设置" @click="router.push(inWorkspace ? `/projects/${id}/settings` : '/settings')">⚙</button></div>
    </div>

    <div class="prototype-shell" :class="{ 'assistant-collapsed': assistantCollapsed, 'no-assistant': !inWorkspace }">
      <nav class="prototype-nav" aria-label="作品导航">
        <template v-if="inWorkspace">
          <template v-for="item in workspaceNav" :key="item.label"><div v-if="item.section" class="prototype-nav-section">{{ item.section }}</div><RouterLink :to="item.to" class="prototype-navbutton" :class="{ active: isActive(item) }"><span class="ico">{{ item.icon }}</span><span>{{ item.label }}</span><span v-if="item.count" class="prototype-navcount">{{ item.count }}</span></RouterLink></template>
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

      <main ref="workspaceEl" class="prototype-workspace"><WriteLockBanner :locked="inWorkspace && store.writeLocked" /><div v-if="inWorkspace && store.loadError" class="prototype-panel"><strong>暂时无法读取作品</strong><p>{{ productErrorMessage(store.loadError, '请返回作品库后重试。') }}</p><button class="prototype-btn" type="button" @click="store.refresh">重试</button></div><div v-else-if="inWorkspace && store.loading && !store.overview" class="prototype-panel">正在读取作品记录…</div><slot v-else /></main>
      <ProjectAssistantDock v-if="inWorkspace" :collapsed="assistantCollapsed" @toggle="assistantCollapsed = !assistantCollapsed" />
    </div>
  </div>
</template>

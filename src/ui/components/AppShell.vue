<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'
import ProjectSwitcher from './ProjectSwitcher.vue'
import ProjectAssistantDock from './ProjectAssistantDock.vue'
import { phaseLabel } from '../presentation/projectLabels'
import { productErrorMessage } from '../presentation/productLanguage'

const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const library = useProjectLibraryStore()
const assistantCollapsed = ref(false)
const id = computed(() => store.currentProjectId ?? (typeof route.params.id === 'string' ? route.params.id : ''))
const inWorkspace = computed(() => route.meta.scope === 'project' && !!id.value)

type NavItem = { label: string; icon: string; to: string; matches: string[]; count?: number }
const workspaceNav = computed<NavItem[]>(() => inWorkspace.value ? [
  { label: '总览', icon: '◫', to: `/projects/${id.value}`, matches: ['project-overview'] },
  { label: '创作', icon: '✎', to: `/projects/${id.value}/production`, matches: ['production'] },
  { label: '规划', icon: '◇', to: `/projects/${id.value}/directions`, matches: ['directions', 'market', 'story-setup', 'opening-ready'] },
  { label: '当前卷', icon: '▥', to: `/projects/${id.value}/volumes`, matches: ['volumes', 'volume-workspace'] },
  { label: '待处理', icon: '⌁', to: `/projects/${id.value}/decisions`, matches: ['decisions', 'history'], count: store.pendingDecisionCount },
  { label: '长篇健康', icon: '♡', to: `/projects/${id.value}/health`, matches: ['health'] },
  { label: '资料', icon: '▤', to: `/projects/${id.value}/read`, matches: ['read', 'graph'] },
  { label: '完结准备', icon: '◯', to: `/projects/${id.value}/completion`, matches: ['completion'] },
] : [])
const routeTitle = computed(() => {
  if (route.name === 'new-project') return '新建作品'
  if (route.name === 'settings') return '应用设置'
  if (route.name === 'projects') return '作品库'
  return store.project?.title ?? '当前作品'
})
const isActive = (item: NavItem) => item.matches.includes(String(route.name))
// 冻结兼容：长篇健康继续使用 /health；版本与变更记录保留 match: 'history'，但统一归入待处理工作区。
</script>

<template>
  <div class="app-shell" :class="{ 'has-assistant': inWorkspace, 'assistant-is-collapsed': assistantCollapsed }">
    <aside class="sidebar">
      <button class="brand" type="button" @click="router.push('/projects')">
        <span class="brand-mark">墨</span>
        <span class="brand-copy"><strong>墨衡</strong><span>长篇小说创作工作台</span></span>
      </button>

      <nav class="nav-group" aria-label="作品导航">
        <div class="nav-label">作品</div>
        <RouterLink to="/projects" class="nav-item" :class="{ active: route.name === 'projects' }">
          <span class="nav-icon">▦</span><span>作品库</span><span v-if="library.projection" class="nav-count neutral">{{ library.projection.totals.active }}</span>
        </RouterLink>
        <RouterLink to="/projects/new" class="nav-item" :class="{ active: route.name === 'new-project' }">
          <span class="nav-icon">＋</span><span>新建作品</span>
        </RouterLink>
      </nav>

      <nav v-if="workspaceNav.length" class="nav-group" aria-label="当前作品导航">
        <div class="nav-label">当前作品</div>
        <RouterLink v-for="item in workspaceNav" :key="item.label" :to="item.to" class="nav-item" :class="{ active: isActive(item) }">
          <span class="nav-icon">{{ item.icon }}</span><span>{{ item.label }}</span><span v-if="item.count" class="nav-count">{{ item.count }}</span>
        </RouterLink>
      </nav>

      <nav class="nav-group nav-group-bottom" aria-label="作品设置">
        <RouterLink :to="inWorkspace ? `/projects/${id}/settings` : '/settings'" class="nav-item" :class="{ active: route.name === 'settings' || route.name === 'project-settings' }">
          <span class="nav-icon">⚙</span><span>{{ inWorkspace ? '作品设置' : '应用设置' }}</span>
        </RouterLink>
      </nav>

      <div class="sidebar-foot">
        <div v-if="inWorkspace && store.project" class="workspace-mini-card">
          <div class="workspace-mini-head"><strong>{{ store.project.title }}</strong><span class="badge pine">{{ store.project.progress }}%</span></div>
          <div class="progress"><span :style="{ width: `${store.project.progress}%` }" /></div>
          <div class="muted workspace-mini-phase">{{ phaseLabel(store.project.phase) }}</div>
        </div>
        <div v-else-if="library.projection" class="library-mini-summary">
          <span><b>{{ library.projection.totals.active }}</b> 个活跃作品</span><span><b>{{ library.projection.totals.waitingDecision }}</b> 个等待处理</span>
        </div>
        <div class="demo-note">正式状态来自当前作品记录；助手只查询和协调，不直接改动正式事实。</div>
      </div>
    </aside>

    <main class="main-shell">
      <header class="topbar">
        <div class="topbar-project">
          <ProjectSwitcher v-if="inWorkspace" />
          <template v-else><strong>{{ routeTitle }}</strong><span v-if="route.name === 'projects' && library.projection" class="muted topbar-meta">{{ library.projection.totals.active }} 个活跃 · {{ library.projection.totals.archived }} 个归档</span></template>
        </div>
        <div class="topbar-actions">
          <span v-if="inWorkspace && store.project" class="topbar-stage">{{ phaseLabel(store.project.phase) }}</span>
          <RouterLink v-if="inWorkspace && store.pendingDecisionCount" :to="`/projects/${id}/decisions`" class="btn warning small">待处理 {{ store.pendingDecisionCount }}</RouterLink>
          <RouterLink to="/projects/new" class="btn primary small">＋ 新建</RouterLink>
          <button v-if="inWorkspace" class="icon-btn assistant-toggle" type="button" :aria-label="assistantCollapsed ? '展开项目助手' : '折叠项目助手'" @click="assistantCollapsed = !assistantCollapsed">{{ assistantCollapsed ? '‹' : '›' }}</button>
          <RouterLink :to="inWorkspace ? `/projects/${id}/settings` : '/settings'" class="icon-btn" aria-label="设置">⚙</RouterLink>
        </div>
      </header>

      <div v-if="inWorkspace && store.loadError" class="page"><div class="banner danger"><div><div class="banner-title">无法读取作品</div><div class="banner-copy">{{ productErrorMessage(store.loadError, '暂时无法读取作品，请返回作品库重试。') }}</div></div><div class="actions"><button class="btn small" @click="router.push('/projects')">返回作品库</button><button class="btn small" @click="store.refresh">重试</button></div></div></div>
      <div v-else-if="inWorkspace && store.loading && !store.overview" class="page"><div class="skeleton" style="height:32px;width:220px" /><div class="skeleton" style="height:180px;margin-top:20px" /><div class="metric-grid" style="margin-top:14px"><div v-for="i in 4" :key="i" class="skeleton" style="height:88px" /></div></div>
      <slot v-else />
    </main>

    <ProjectAssistantDock v-if="inWorkspace" :collapsed="assistantCollapsed" @toggle="assistantCollapsed = !assistantCollapsed" />

    <nav class="mobile-nav" aria-label="移动端导航">
      <template v-if="inWorkspace">
        <RouterLink :to="`/projects/${id}`" :class="{ active: route.name === 'project-overview' }"><span>◫</span><span>总览</span></RouterLink>
        <RouterLink :to="`/projects/${id}/production`" :class="{ active: route.name === 'production' }"><span>✎</span><span>创作</span></RouterLink>
        <RouterLink :to="`/projects/${id}/decisions`" :class="{ active: route.name === 'decisions' }"><span>⌁</span><span>待处理</span></RouterLink>
        <RouterLink :to="`/projects/${id}/read`" :class="{ active: route.name === 'read' }"><span>▤</span><span>资料</span></RouterLink>
        <button type="button" class="mobile-assistant-button" @click="assistantCollapsed = false"><span>墨</span><span>助手</span></button>
      </template>
      <template v-else>
        <RouterLink to="/projects" :class="{ active: route.name === 'projects' }"><span>▦</span><span>作品库</span></RouterLink>
        <RouterLink to="/projects/new" :class="{ active: route.name === 'new-project' }"><span>＋</span><span>新建</span></RouterLink>
        <RouterLink to="/settings" :class="{ active: route.name === 'settings' }"><span>⚙</span><span>设置</span></RouterLink>
      </template>
    </nav>
  </div>
</template>

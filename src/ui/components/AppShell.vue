<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'
import ProjectSwitcher from './ProjectSwitcher.vue'
import { phaseLabel } from '../presentation/projectLabels'
import { productErrorMessage } from '../presentation/productLanguage'

const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const library = useProjectLibraryStore()
const id = computed(() => store.currentProjectId ?? (typeof route.params.id === 'string' ? route.params.id : ''))
const inWorkspace = computed(() => route.meta.scope === 'project' && !!id.value)

type NavItem = { label:string; icon:string; to:string; match:string; count?:number; disabled?:boolean }
const workspaceNav = computed<NavItem[]>(() => inWorkspace.value ? [
  { label: '总览', icon: '◫', to: `/projects/${id.value}`, match: 'project-overview' },
  { label: '市场与平台策略', icon: '↗', to: `/projects/${id.value}/market`, match: 'market' },
  { label: '故事方向', icon: '◇', to: `/projects/${id.value}/directions`, match: 'directions' },
  { label: '故事核心', icon: '◎', to: `/projects/${id.value}/setup`, match: 'story-setup' },
  { label: '开篇准备', icon: '◇', to: `/projects/${id.value}/opening`, match: 'opening-ready' },
  { label: '项目助手', icon: '?', to: `/projects/${id.value}/assistant`, match: 'assistant' },
  { label: '创作中心', icon: '◉', to: `/projects/${id.value}/production`, match: 'production' },
  { label: '卷管理', icon: '▥', to: `/projects/${id.value}/volumes`, match: 'volumes' },
  { label: '决策收件箱', icon: '⌁', to: `/projects/${id.value}/decisions`, match: 'decisions', count: store.pendingDecisionCount },
  { label: '成品阅读', icon: '▤', to: `/projects/${id.value}/read`, match: 'read' },
  { label: '作品图谱', icon: '⌘', to: `/projects/${id.value}/graph`, match: 'graph' },
  { label: '长篇健康', icon: '♡', to: '', match: 'health', disabled: true },
  { label: '版本与变更记录', icon: '↺', to: `/projects/${id.value}/history`, match: 'history' }
] : [])
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <button class="brand" style="border:0;background:transparent;text-align:left" @click="router.push('/projects')">
        <span class="brand-mark">墨</span>
        <span class="brand-copy"><strong>墨衡</strong><span>AI 小说生产系统</span></span>
      </button>

      <nav class="nav-group" aria-label="应用导航">
        <div class="nav-label">作品</div>
        <RouterLink to="/projects" class="nav-item" :class="{active:route.name==='projects'}"><span class="nav-icon">▦</span><span>作品库</span><span v-if="library.projection" class="nav-count neutral">{{ library.projection.totals.active }}</span></RouterLink>
        <RouterLink to="/projects/new" class="nav-item" :class="{active:route.name==='new-project'}"><span class="nav-icon">＋</span><span>新建作品</span></RouterLink>
      </nav>

      <nav v-if="workspaceNav.length" class="nav-group" aria-label="作品工作区">
        <div class="nav-label">当前作品</div>
        <template v-for="item in workspaceNav" :key="item.match">
          <RouterLink v-if="!item.disabled" :to="item.to" class="nav-item" :class="{active: route.name === item.match || (item.match === 'volumes' && route.name === 'volume-workspace')}">
            <span class="nav-icon">{{ item.icon }}</span>
            <span>{{ item.label }}</span>
            <span v-if="item.count" class="nav-count">{{ item.count }}</span>
          </RouterLink>
          <div v-else class="nav-item disabled" aria-disabled="true">
            <span class="nav-icon">{{ item.icon }}</span>
            <span>{{ item.label }}</span>
            <span class="nav-count neutral">尚未开放</span>
          </div>
        </template>
      </nav>

      <nav class="nav-group">
        <div class="nav-label">偏好与工具</div>
        <RouterLink :to="inWorkspace ? `/projects/${id}/settings` : '/settings'" class="nav-item" :class="{active: route.name === 'settings' || route.name === 'project-settings'}"><span class="nav-icon">⚙</span><span>{{ inWorkspace ? '项目设置' : '应用设置' }}</span></RouterLink>
      </nav>

      <div class="sidebar-foot">
        <div v-if="inWorkspace && store.project" class="card pad" style="padding:12px">
          <div style="display:flex;align-items:center;justify-content:space-between;gap:8px">
            <strong style="font-size:12px">{{ store.project.title }}</strong>
            <span class="badge pine">{{ store.project.progress }}%</span>
          </div>
          <div class="progress" style="margin:9px 0 7px"><span :style="{width: `${store.project.progress}%`}" /></div>
          <div class="muted" style="font-size:11px">{{ phaseLabel(store.project.phase) }}</div>
        </div>
        <div v-else-if="library.projection" class="library-mini-summary">
          <span><b>{{ library.projection.totals.active }}</b> 个活跃作品</span>
          <span><b>{{ library.projection.totals.waitingDecision }}</b> 个等待决定</span>
        </div>
        <div class="demo-note">正式状态由本地项目记录确认，切换作品不会混用数据。</div>
      </div>
    </aside>

    <main class="main-shell">
      <header class="topbar">
        <div class="topbar-project">
          <ProjectSwitcher v-if="inWorkspace" />
          <template v-else>
            <strong>{{ route.name === 'new-project' ? '新建作品' : route.name === 'settings' ? '应用设置' : '作品库' }}</strong>
            <span v-if="route.name==='projects' && library.projection" class="muted no-wrap" style="font-size:11px">{{ library.projection.totals.active }} 个活跃 · {{ library.projection.totals.archived }} 个归档</span>
          </template>
        </div>
        <div class="topbar-actions">
          <button v-if="inWorkspace" class="btn soft small" @click="store.advancedMode = !store.advancedMode">
            <span>{{ store.advancedMode ? '关闭高级详情' : '高级详情' }}</span>
          </button>
          <RouterLink v-if="inWorkspace && store.pendingDecisionCount" :to="`/projects/${id}/decisions`" class="btn warning small">
            <span>待决定 {{ store.pendingDecisionCount }}</span>
          </RouterLink>
          <RouterLink to="/projects/new" class="btn primary small"><span>＋ 新建作品</span></RouterLink>
          <RouterLink :to="inWorkspace ? `/projects/${id}/settings` : '/settings'" class="icon-btn" aria-label="设置">⚙</RouterLink>
        </div>
      </header>

      <div v-if="inWorkspace && store.loadError" class="page"><div class="banner danger"><div><div class="banner-title">无法读取作品</div><div class="banner-copy">{{ productErrorMessage(store.loadError, '暂时无法读取作品，请返回作品库重试。') }}</div></div><div class="actions"><button class="btn small" @click="router.push('/projects')">返回作品库</button><button class="btn small" @click="store.refresh">重试</button></div></div></div>
      <div v-else-if="inWorkspace && store.loading && !store.overview" class="page"><div class="skeleton" style="height:32px;width:220px"/><div class="skeleton" style="height:180px;margin-top:20px"/><div class="metric-grid" style="margin-top:14px"><div v-for="i in 4" :key="i" class="skeleton" style="height:88px"/></div></div>
      <slot v-else />
    </main>

    <nav class="mobile-nav" aria-label="移动端导航">
      <template v-if="inWorkspace">
        <RouterLink :to="`/projects/${id}`" :class="{active: route.name==='project-overview'}"><span>◫</span><span>概览</span></RouterLink>
        <RouterLink :to="`/projects/${id}/production`" :class="{active: route.name==='production'}"><span>◉</span><span>制片</span></RouterLink>
        <RouterLink :to="`/projects/${id}/decisions`" :class="{active: route.name==='decisions'}"><span>⌁</span><span>决策</span></RouterLink>
        <RouterLink :to="`/projects/${id}/read`" :class="{active: route.name==='read'}"><span>▤</span><span>阅读</span></RouterLink>
        <RouterLink to="/projects"><span>▦</span><span>作品库</span></RouterLink>
      </template>
      <template v-else>
        <RouterLink to="/projects" :class="{active: route.name==='projects'}"><span>▦</span><span>作品库</span></RouterLink>
        <RouterLink to="/projects/new" :class="{active: route.name==='new-project'}"><span>＋</span><span>新建</span></RouterLink>
        <RouterLink to="/settings" :class="{active: route.name==='settings'}"><span>⚙</span><span>设置</span></RouterLink>
      </template>
    </nav>
  </div>
</template>

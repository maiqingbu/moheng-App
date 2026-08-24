<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'
import { useProjectStore } from '../stores/projectStore'
import { phaseLabel, runStateLabel } from '../presentation/projectLabels'

const router = useRouter()
const library = useProjectLibraryStore()
const projectStore = useProjectStore()
const open = ref(false)
const query = ref('')

const projects = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return library.activeProjects.filter(item => !needle || `${item.title} ${item.genre} ${item.platform}`.toLowerCase().includes(needle))
})

function selectProject(id: string) {
  open.value = false
  query.value = ''
  router.push(`/projects/${id}`)
}
</script>

<template>
  <div class="project-switcher">
    <button class="project-switcher-trigger" type="button" @click="open=!open">
      <span class="project-switcher-dot" />
      <span class="project-switcher-copy">
        <strong>{{ projectStore.project?.title ?? '选择作品' }}</strong>
        <span>{{ projectStore.project ? phaseLabel(projectStore.project.phase) : '作品库' }}</span>
      </span>
      <span class="project-switcher-caret">⌄</span>
    </button>
    <div v-if="open" class="project-switcher-panel">
      <div class="project-switcher-search"><input v-model="query" class="input" placeholder="搜索作品…" autofocus /></div>
      <button class="switcher-action" @click="open=false;router.push('/projects')"><span>▦</span><span><strong>全部作品</strong><small>打开作品库</small></span></button>
      <button class="switcher-action" @click="open=false;router.push('/projects/new')"><span>＋</span><span><strong>新建作品</strong><small>从想法或市场机会开始</small></span></button>
      <div class="switcher-divider" />
      <div class="switcher-list">
        <button v-for="item in projects" :key="item.id" class="switcher-project" :class="{active:item.id===projectStore.currentProjectId}" @click="selectProject(item.id)">
          <span class="mini-cover">{{ item.title.slice(0,1) }}</span>
          <span class="switcher-project-copy"><strong>{{ item.title }}</strong><small>{{ runStateLabel(item.runState) }} · {{ item.platform }}</small></span>
          <span v-if="item.pendingDecisions" class="nav-count">{{ item.pendingDecisions }}</span>
        </button>
        <div v-if="!projects.length" class="empty" style="padding:20px 10px">没有匹配的作品</div>
      </div>
    </div>
    <button v-if="open" class="project-switcher-scrim" aria-label="关闭作品切换器" @click="open=false" />
  </div>
</template>

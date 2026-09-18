<script setup lang="ts">
import { onMounted, watch } from 'vue'
import { RouterView, useRoute, useRouter } from 'vue-router'
import AppShell from './ui/components/AppShell.vue'
import { useProjectStore } from './ui/stores/projectStore'
import { useProjectLibraryStore } from './ui/stores/projectLibraryStore'

const route = useRoute()
const router = useRouter()
const projectStore = useProjectStore()
const libraryStore = useProjectLibraryStore()

async function syncRouteProject() {
  const projectId = typeof route.params.id === 'string' ? route.params.id : ''
  if (route.meta.scope === 'project' && projectId) {
    await projectStore.openProject(projectId)
  } else if (route.meta.scope === 'library') {
    projectStore.clearProject()
  }
}

onMounted(async () => {
  if (import.meta.env.VITE_REAL_LLM_UI_GATE === 'true' && route.name !== 'real-llm-gate') {
    await router.replace('/real-llm-gate')
  }
  await libraryStore.bootstrap()
  await syncRouteProject()
})

watch(() => [route.name, route.params.id], syncRouteProject)
</script>

<template>
  <AppShell>
    <RouterView />
  </AppShell>
</template>

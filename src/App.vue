<script setup lang="ts">
import { onMounted, watch } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import AppShell from './ui/components/AppShell.vue'
import { useProjectStore } from './ui/stores/projectStore'
import { useProjectLibraryStore } from './ui/stores/projectLibraryStore'

const route = useRoute()
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

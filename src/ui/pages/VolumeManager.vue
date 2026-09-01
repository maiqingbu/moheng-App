<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const router = useRouter()
const items = computed(() => store.volumes?.items ?? [])
const state = computed(() => store.commandState('volume-build'))
const signingPassed = computed(() => store.project?.signingStatus === 'PASS')
const signingLabel = (value: boolean) => value ? '已通过' : '尚未通过'
const volumeStatusLabel = (value: string) => ({ LOCKED: '待解锁', CLOSED: '已关闭', PRODUCING: '生产中', PAUSED: '已暂停', PLANNING: '规划中' } as Record<string, string>)[value] ?? '待确认'
onMounted(() => void store.loadVolumes())
async function build() {
  const current = items.value.find(volume => volume.status !== 'CLOSED') ?? items.value[0]
  if (!current) return
  await store.runCommand('volume-build', { type: 'planning_create_volume_intent', payload: { volumeId: current.id } })
  await store.runCommand('volume-build', { type: 'planning_create_volume_plan', payload: { volumeId: current.id } })
  await store.loadVolumes(true)
}
</script>

<template>
  <div class="page">
    <PageHeader title="卷管理" subtitle="BookPlan 只保存全书战略；详细 VolumePlan 按当前卷创建，不一次生成整书章纲。">
      <button class="btn primary" :disabled="!signingPassed" @click="build">建立当前卷规划</button>
    </PageHeader>
    <CommandFeedback :state="state" />

    <div v-if="!signingPassed" class="banner warning section">
      <div><div class="banner-title">卷级生产尚未解锁</div><div class="banner-copy">开篇评估通过并生成正式回执后，才能建立详细卷计划和启动后续长篇生产。</div></div>
    </div>

    <div class="section" style="display:grid;gap:12px">
      <article v-for="volume in items" :key="volume.id" class="card pad">
        <div class="card-head">
          <div><span class="badge" :class="volume.health === 'risk' ? 'danger' : volume.health === 'attention' ? 'warning' : 'success'">第 {{ volume.number }} 卷 · {{ volumeStatusLabel(volume.status) }}</span><h2 class="card-title" style="margin-top:8px">{{ volume.title }}</h2><p class="card-subtitle">{{ volume.goal }}</p></div>
          <button class="btn" @click="router.push(`/projects/${store.currentProjectId}/volumes/${volume.id}`)">进入卷工作区</button>
        </div>
          <div class="muted" style="font-size:11px;margin-top:8px">已提交正文 {{ volume.wordCount.toLocaleString() }} 字；进入卷工作区查看任务安排、实际进展与动态规划压力。</div>
      </article>
    </div>
  </div>
</template>

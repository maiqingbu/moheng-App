<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import MetricCard from '../components/MetricCard.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const router = useRouter()
const opening = computed(() => store.openingReady)
const openingProgress = computed(() => opening.value?.openingProgress)
const approvalState = computed(() => store.commandState('opening-ready'))
const startState = computed(() => store.commandState('opening-start'))

onMounted(() => void store.loadOpeningReady())

async function approveOpening() {
  await store.runCommand('opening-ready', { type: 'opening_ready', payload: { approved: true } })
  await store.loadOpeningReady(true)
}

async function startCreating() {
  const receipt = await store.runCommand('opening-start', { type: 'opening_start_creating', payload: {} })
  if (receipt.finalStatus === 'committed') router.push(`/projects/${store.currentProjectId ?? ''}/production`)
}
</script>

<template>
  <div class="page">
    <PageHeader title="规划 · 开篇准备" subtitle="确认前三章的承诺、边界与第一章目标，然后进入正式创作。">
      <button class="btn" @click="router.push(`/projects/${store.currentProjectId ?? ''}/setup`)">返回故事核心</button>
    </PageHeader>
    <div v-if="!opening" class="skeleton" style="height:180px" />
    <template v-else>
      <div class="banner" :class="opening.ready ? 'success' : 'info'"><div><div class="banner-title">{{ opening.ready ? '开篇已准备完成' : '等待你的确认' }}</div><div class="banner-copy">{{ opening.ready ? '第一章最小规划已经由现有规划责任方生成。' : '确认后才会建立第一章的最小规划。' }}</div></div></div>
      <div class="metric-grid section">
        <MetricCard label="第一章目标" :value="opening.firstChapterGoal" note="只建立当前开篇边界" />
        <MetricCard label="前三章承诺" :value="opening.firstThreeChapterPromise" note="用于检查开篇是否兑现" />
        <MetricCard label="读者期待" :value="opening.readerDesire" note="保留为创作方向" />
        <MetricCard label="当前状态" :value="opening.ready ? '已准备' : '待确认'" :note="opening.firstChapterPlan ? '已生成第一章最小规划' : '尚未生成规划'" />
        <MetricCard label="黄金三章" :value="`第 ${openingProgress?.completed ?? 0} / ${openingProgress?.total ?? 3} 章`" :note="openingProgress?.currentChapter ? `当前第 ${openingProgress.currentChapter} 章` : '下一章尚未准备'" />
      </div>
      <section class="card pad section">
        <div class="card-head"><div><h2 class="card-title">开篇边界</h2><p class="card-subtitle">不在入口阶段生成未来章节，也不跳过正式规划与生产链。</p></div></div>
        <div class="task-row"><strong>前三章暂不揭开</strong><span>{{ opening.forbiddenReveals.join('；') }}</span></div>
        <div class="task-row"><strong>黄金三章进度</strong><span>第 {{ openingProgress?.completed ?? 0 }} / {{ openingProgress?.total ?? 3 }} 章 · 当前第 {{ openingProgress?.currentChapter ?? '—' }} 章</span></div>
        <div class="task-row"><strong>上一章正式提交</strong><span>{{ openingProgress?.previousCommitted ? '是' : '尚无上一章' }} · 下一章{{ openingProgress?.nextPrepared ? '已准备' : '未准备' }}</span></div>
        <div v-if="opening.firstChapterPlan" class="task-row"><strong>第一章规划已存在</strong><span>可以进入正式创作中心继续操作。</span></div>
        <CommandFeedback :state="approvalState" />
        <CommandFeedback :state="startState" />
        <div class="actions" style="justify-content:flex-end;margin-top:16px"><button v-if="!opening.ready" class="btn primary" :disabled="['submitting','accepted','processing'].includes(approvalState.status)" @click="approveOpening">确认开篇准备</button><button v-else class="btn primary" :disabled="['submitting','accepted','processing'].includes(startState.status)" @click="startCreating">进入正式创作</button></div>
      </section>
    </template>
  </div>
</template>

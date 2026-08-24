<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const route = useRoute()
const volumeId = computed(() => String(route.params.volumeId ?? ''))
const detail = computed(() => store.volumeDetail)
const state = computed(() => store.commandState('volume-command'))
const locked = computed(() => detail.value?.volume.status === 'LOCKED')
const canPause = computed(() => detail.value?.volume.status === 'PRODUCING')
const canClose = computed(() => !!detail.value && !locked.value && detail.value.volume.status !== 'CLOSED')

onMounted(() => void store.loadVolumeDetail(volumeId.value))
async function run(type: 'volume_production_start' | 'volume_production_pause' | 'volume_close') {
  await store.runCommand('volume-command', { type, payload: { volumeId: volumeId.value } })
  await store.loadVolumeDetail(volumeId.value, true)
}
</script>

<template>
  <div v-if="detail" class="page">
    <PageHeader :title="`第 ${detail.volume.number} 卷 · ${detail.volume.title}`" :subtitle="detail.volume.goal">
      <button class="btn primary" :disabled="locked" @click="run('volume_production_start')">开始/继续本卷</button>
      <button class="btn" :disabled="!canPause" @click="run('volume_production_pause')">暂停本卷</button>
      <button class="btn" :disabled="!canClose" @click="run('volume_close')">运行卷关闭检查</button>
    </PageHeader>
    <CommandFeedback :state="state" />

    <div v-if="locked" class="banner warning section"><div><div class="banner-title">当前卷仍处于准入锁定</div><div class="banner-copy">开篇评估 PASS 前可以查看全书战略中的卷意图，但不能创建详细卷计划或启动后续正文生产。</div></div></div>

    <div class="metric-grid section">
      <MetricCard label="卷进度" :value="`${detail.volume.progress}%`" :note="`${detail.volume.committedChapters}/${detail.volume.plannedChapters} 章`" />
      <MetricCard label="未完成剧情义务" :value="detail.openObligations" note="关闭前必须完成或正式处置" />
      <MetricCard label="临期伏笔" :value="detail.overdueForeshadows" note="按卷健康门检查" />
      <MetricCard label="下一卷" :value="detail.exitEligibility.ready ? '可解锁' : '未解锁'" :note="detail.nextVolumeEligibility" />
    </div>

    <section class="card pad section">
      <div class="card-head"><div><h2 class="card-title">本卷战术计划</h2><p class="card-subtitle">详细规划只覆盖当前卷；下一卷消费本卷正式退出状态后再展开。</p></div><span class="badge">{{ detail.volume.status }}</span></div>
      <div class="timeline">
        <div v-for="(arc, index) in detail.arcs" :key="arc.id" class="timeline-item" :class="arc.status">
          <div class="timeline-marker">{{ arc.status === 'done' ? '✓' : index + 1 }}</div>
          <div><strong>{{ arc.title }} · {{ arc.chapterRange }}</strong><div class="muted" style="font-size:12px">{{ arc.goal }}</div></div>
        </div>
      </div>
    </section>

    <section v-if="store.advancedMode" class="card pad section">
      <h2 class="card-title">高级详情</h2>
      <dl class="pitch-kv"><div><dt>卷计划版本</dt><dd>{{ detail.planVersion }}</dd></div><div><dt>卷约束版本</dt><dd>{{ detail.contractVersion }}</dd></div><div><dt>规划状态账本</dt><dd>{{ detail.stateLedgerVersion }}</dd></div></dl>
    </section>
  </div>
</template>

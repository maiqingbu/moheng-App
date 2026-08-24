<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const router = useRouter()
const health = computed(() => store.health)
onMounted(() => void store.loadHealth())
</script>

<template>
  <div v-if="health" class="page">
    <PageHeader title="长篇健康" subtitle="用创作语言提示长期风险；技术 ID 和依赖类型只在高级详情中显示。">
      <button class="btn" @click="store.loadHealth(true)">重新检查</button>
    </PageHeader>

    <div class="metric-grid section">
      <MetricCard label="长期健康" :value="`${health.score} 分`" :note="health.level === 'risk' ? '存在高影响风险' : health.level === 'attention' ? '总体稳定，有关注项' : '状态稳定'" />
      <MetricCard label="终局推进" :value="`${health.progressToEnding}%`" :note="`目标 ${health.targetLength}`" />
      <MetricCard label="未解决剧情义务" :value="health.counters.openObligations" :note="`${health.counters.overdueObligations} 项超期`" />
      <MetricCard label="待回收伏笔" :value="health.counters.openForeshadows" :note="`${health.counters.overdueForeshadows} 项临期/超期`" />
    </div>

    <section class="section">
      <div class="section-head"><div><h2>需要关注</h2><div class="muted" style="font-size:12px">检查时间 {{ health.checkedAt }}</div></div></div>
      <div style="display:grid;gap:10px">
        <article v-for="issue in health.issues.items" :key="issue.id" class="card pad">
          <div class="card-head">
            <div>
              <span class="badge" :class="issue.severity === 'risk' ? 'danger' : issue.severity === 'attention' ? 'warning' : 'info'">{{ issue.category }}</span>
              <h3 style="margin:8px 0 4px">{{ issue.title }}</h3>
              <p class="muted">{{ issue.detail }}</p>
            </div>
            <button v-if="issue.category === '修改影响'" class="btn small" @click="router.push(`/projects/${store.currentProjectId}/history`)">去做影响分析</button>
          </div>
          <div class="banner info"><div><div class="banner-title">建议</div><div class="banner-copy">{{ issue.suggestedAction }} · {{ issue.location }}</div></div></div>
          <dl v-if="store.advancedMode && issue.advanced" class="pitch-kv" style="margin-top:10px">
            <div v-if="issue.advanced.obligationId"><dt>义务 ID</dt><dd>{{ issue.advanced.obligationId }}</dd></div>
            <div v-if="issue.advanced.dependencyType"><dt>依赖类型</dt><dd>{{ issue.advanced.dependencyType }}</dd></div>
            <div v-if="issue.advanced.affectedEstimate"><dt>预计影响</dt><dd>{{ issue.advanced.affectedEstimate }}</dd></div>
          </dl>
        </article>
        <div v-if="!health.issues.items.length" class="empty">当前没有需要处理的长期风险。</div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const strategy = computed(() => store.marketStrategy)
onMounted(() => void store.loadMarket())
</script>

<template>
  <div v-if="strategy" class="page">
    <PageHeader title="市场与平台策略" subtitle="市场信息与作品记录分开，只用于机会判断和授权范围内的策略校准。" />
    <div class="metric-grid section">
      <MetricCard label="目标平台" :value="strategy.targetPlatform" :note="strategy.platformPolicyVersion" />
      <MetricCard label="策略置信度" :value="`${strategy.confidence}%`" :note="`快照 ${strategy.snapshotCollectedAt}`" />
      <MetricCard label="最近复核" :value="strategy.lastPulseAt" :note="`下次：${strategy.nextReview}`" />
      <MetricCard label="当前策略" :value="strategy.strategyTitle" :note="strategy.readerPromise" />
    </div>
    <div class="market-grid section">
      <article v-for="item in strategy.opportunities" :key="item.id" class="market-card">
        <span class="badge">{{ item.kind }}</span>
        <h3>{{ item.title }}</h3>
        <div class="market-meta">{{ item.platform }} · {{ item.direction }}</div>
        <div class="score-grid">
          <div class="score"><b>{{ item.demand }}</b><span>需求</span></div>
          <div class="score"><b>{{ item.growth }}</b><span>增长</span></div>
          <div class="score"><b>{{ item.originalitySpace }}</b><span>差异空间</span></div>
        </div>
        <p>{{ item.recommendWhy }}</p>
        <small class="muted">{{ item.source }} · {{ item.collectedAt }}</small>
      </article>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import type { ContinuityThreadViewProjection } from '../../application/contracts'
import { continuityClassLabel, continuityDispositionLabel, resumeConditionLabel } from '../presentation/continuity'

const props = defineProps<{
  threads: ContinuityThreadViewProjection[]
  truncated: boolean
  advanced: boolean
}>()
const emit = defineEmits<{ 'request-change': [thread: ContinuityThreadViewProjection] }>()
const visibleThreads = computed(() => props.threads.filter(thread => ['OPEN', 'SUSPENDED'].includes(thread.logicalStatus)))

function readableSource(thread: ContinuityThreadViewProjection): string {
  const evidence = thread.evidenceRefs.find(item => item.quote?.trim())
  if (evidence?.quote) return `来源章节正文（版本 ${evidence.version}）：“${evidence.quote}”`
  return thread.sourceEventRef ? '来源事件已记录，暂无可展示的引文。' : '来源记录已登记，暂无可展示的引文。'
}

function rawSourceRefs(thread: ContinuityThreadViewProjection): string {
  return JSON.stringify(thread.sourceRefs)
}

function rangeLabel(evidence: ContinuityThreadViewProjection['evidenceRefs'][number]): string {
  if (evidence.rangeStart === null || evidence.rangeStart === undefined || evidence.rangeEnd === null || evidence.rangeEnd === undefined) return '未记录字符范围'
  return `字符 ${evidence.rangeStart}–${evidence.rangeEnd}`
}
</script>

<template>
  <section class="card pad section continuity-projection" data-continuity-projection="REALITY">
    <div class="card-head">
      <div>
        <h2 class="card-title">未完成剧情线</h2>
        <p class="card-subtitle">只读呈现已记录的剧情责任、来源和当前处置。</p>
      </div>
      <span class="badge">REALITY · 投影</span>
    </div>
    <p v-if="truncated" class="banner warning" role="status">仅显示部分，已截断</p>
    <p v-if="!visibleThreads.length" class="muted">当前没有 OPEN 或 SUSPENDED 的剧情线。</p>
    <article v-for="thread in visibleThreads" :key="thread.threadId" class="continuity-thread">
      <div class="card-head">
        <div>
          <strong>{{ continuityClassLabel(thread.threadClass) }} · {{ thread.logicalStatus }}</strong>
          <p class="muted continuity-description">{{ thread.description || '暂无说明。' }}</p>
        </div>
        <span class="badge">{{ thread.authorityLevel }} 级</span>
      </div>
      <dl class="pitch-kv">
        <div><dt>来源事件</dt><dd>{{ readableSource(thread) }}</dd></div>
        <div><dt>兑现条件</dt><dd>{{ resumeConditionLabel(thread.resumeCondition) }}</dd></div>
        <div><dt>当前处置</dt><dd>{{ continuityDispositionLabel(thread.currentDisposition) }}</dd></div>
      </dl>
      <details class="continuity-evidence">
        <summary>证据引文（{{ thread.evidenceRefs.filter(item => item.quote).length }}）</summary>
        <blockquote v-for="(evidence, index) in thread.evidenceRefs" :key="`${evidence.artifactId}:${evidence.version}:${index}`">
          <p v-if="evidence.quote">“{{ evidence.quote }}”</p>
          <p v-else class="muted">未提供可展示的引文。</p>
          <small v-if="advanced" class="muted">{{ evidence.artifactId }} · v{{ evidence.version }} · {{ rangeLabel(evidence) }}</small>
        </blockquote>
      </details>
      <dl v-if="advanced" class="pitch-kv advanced-continuity-refs">
        <div><dt>sourceEventRef</dt><dd><code>{{ thread.sourceEventRef ?? '未提供' }}</code></dd></div>
        <div><dt>sourceRefs</dt><dd><code>{{ rawSourceRefs(thread) }}</code></dd></div>
      </dl>
      <div v-if="['A', 'B'].includes(thread.authorityLevel)" class="actions">
        <button class="btn" type="button" @click="emit('request-change', thread)">发起变更</button>
      </div>
    </article>
  </section>
</template>

<style scoped>
.continuity-projection { border-top: 3px solid var(--accent, #8c6b43); }
.continuity-thread { display: grid; gap: 10px; padding: 14px 0; border-top: 1px solid var(--border, #dedbd3); }
.continuity-description { margin: 6px 0 0; }
.continuity-evidence blockquote { margin: 8px 0; padding: 8px 12px; border-left: 3px solid var(--border, #dedbd3); }
.advanced-continuity-refs code { overflow-wrap: anywhere; }
</style>

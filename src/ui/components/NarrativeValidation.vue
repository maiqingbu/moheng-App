<script setup lang="ts">
import type { NarrativeValidationProjection } from '../../application/contracts'
import { productErrorMessage } from '../presentation/productLanguage'

defineProps<{ validation?: NarrativeValidationProjection; advanced?: boolean }>()
const checkLabel = (status: string) => ({
  COMPLETE: '检查完整', OVER_BUDGET: '必需上下文超出预算', INCOMPLETE: '检查不完整',
  UNAVAILABLE: '检查依据暂不可用', UNRESOLVED: '存在未决问题', STALE: '审计依据已变化',
} as Record<string, string>)[status] ?? '检查状态待确认'
</script>

<template>
  <section class="card pad section" data-narrative-validation aria-live="polite">
    <div class="card-head"><div><h2 class="card-title">上下文与正文审计</h2><p class="card-subtitle">依据当前章节的必需资料和正文版本检查；没有检查结果不等于已通过。</p></div></div>
    <p v-if="!validation" class="muted">暂未提供完整校验详情，不能据此确认上下文完整或正文审计通过。</p>
    <template v-else>
      <p v-if="!validation.canExecute" class="banner warning">当前校验不允许继续生产，请先处理检查结果。</p>
      <div class="task-row"><strong>必需上下文</strong><span>{{ validation.context ? checkLabel(validation.context.status) : '尚未提供检查结果' }}</span></div>
      <p v-if="validation.context?.reason">{{ productErrorMessage(validation.context.reason, '请核对本章必需资料与预算。') }}</p>
      <div class="task-row"><strong>正文审计</strong><span>{{ validation.audit ? checkLabel(validation.audit.status) : '尚未提供检查结果' }}</span></div>
      <p v-if="validation.audit?.reason">{{ productErrorMessage(validation.audit.reason, '请核对最终正文版本及其审计依据。') }}</p>
      <dl v-if="advanced" class="pitch-kv">
        <div><dt>HARD 来源数 / token 预算</dt><dd>{{ validation.context?.hardSourceCount ?? '未提供' }} / {{ validation.context?.hardTokens ?? '未提供' }} / {{ validation.context?.hardTokenLimit ?? '未提供' }}</dd></div>
        <div><dt>审计绑定的候选版本</dt><dd>{{ validation.audit?.candidateRef?.artifactId ?? '未提供' }} · {{ validation.audit?.candidateRef?.version ?? '—' }}</dd></div>
        <div><dt>来源引用</dt><dd>{{ validation.context?.sourceRefs?.join('、') || '未提供' }}</dd></div>
        <div><dt>审计证据</dt><dd>{{ validation.audit?.evidenceRefs?.join('、') || '未提供' }}</dd></div>
      </dl>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'

const route = useRoute()
const router = useRouter()
const feature = computed(() => route.name === 'health'
  ? { title: '长篇健康', description: '该工作区尚未开放，当前不会读取或生成健康评分。' }
  : { title: '版本与变更记录', description: '该工作区尚未开放，当前不会执行版本变更或事实修订。' })
</script>

<template>
  <div class="page">
    <PageHeader :title="feature.title" subtitle="当前版本保留信息架构入口，但未开放对应能力。" />
    <section class="card pad section">
      <h2 class="card-title">该功能尚未开放。</h2>
      <p class="card-subtitle">{{ feature.description }}</p>
      <button class="btn" style="margin-top:16px" @click="router.push(`/projects/${String(route.params.id ?? '')}`)">返回作品总览</button>
    </section>
  </div>
</template>

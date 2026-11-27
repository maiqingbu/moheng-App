<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const router = useRouter()
const setup = computed(() => store.storySetup)
const state = computed(() => store.commandState('story-setup-save'))
const loadError = ref('')
const core = reactive({
  protagonist: '',
  desire: '',
  mechanism: '',
  conflict: '',
  readerPromise: '',
  worldRules: '',
  forbiddenReveals: '',
  style: '',
  expandableDirections: '',
})

onMounted(async () => {
  try {
    const value = await store.loadStorySetup()
    const storyCore = value?.storyCore
    if (!storyCore) return
    for (const key of Object.keys(core) as Array<keyof typeof core>) {
      const value = storyCore[key]
      if (typeof value === 'string') core[key] = value
      else if (Array.isArray(value)) core[key] = value.filter((item): item is string => typeof item === 'string').join('\n')
    }
  } catch { loadError.value = '故事核心暂时无法读取，请重新打开本页后再保存。' }
})

const lines = (value: string) => value.split('\n').map(item => item.trim()).filter(Boolean)

async function saveAndContinue() {
  if (loadError.value || !core.protagonist.trim() || !core.readerPromise.trim() || ['submitting','accepted','processing'].includes(state.value.status)) return
  try {
    const receipt = await store.runCommand('story-setup-save', {
      type: 'story_setup_save',
      payload: {
        storyCore: {
          ...setup.value?.storyCore,
          ...core,
          worldRules: lines(core.worldRules),
          forbiddenReveals: lines(core.forbiddenReveals),
          expandableDirections: lines(core.expandableDirections),
        },
      },
    })
    if (receipt.finalStatus === 'committed' && store.commandState('story-setup-save').confirmation !== 'pending') router.push(`/projects/${store.currentProjectId ?? ''}/craft`)
  } catch { /* CommandFeedback preserves the command outcome. */ }
}
</script>

<template>
  <div class="page">
    <PageHeader title="规划 · 故事核心" subtitle="确认主角、欲望、核心机制与读者承诺，再决定是否进入开篇准备。">
      <button class="btn" @click="router.push(`/projects/${store.currentProjectId ?? ''}/directions`)">返回故事方向</button>
    </PageHeader>

    <div v-if="loadError" class="banner warning" role="alert">{{ loadError }}</div>
    <div v-else-if="!setup || setup.status === 'NOT_SELECTED'" class="banner warning">
      <div><div class="banner-title">请先明确选择故事方向</div><div class="banner-copy">系统不会替你选择，也不会在没有明确选择时建立正式故事核心。</div></div>
      <button class="btn small" @click="router.push(`/projects/${store.currentProjectId ?? ''}/directions`)">去选择</button>
    </div>

    <section v-else class="card pad section">
      <div class="card-head"><div><h2 class="card-title">把方向变成可持续的故事核心</h2><p class="card-subtitle">这里只记录当前确认的核心边界，不提前生成整部作品的章节。</p></div><span class="badge pine">已明确选择</span></div>
      <div class="setting-grid" style="margin-top:18px">
        <label class="field"><span class="field-label">主角</span><input v-model="core.protagonist" class="input" /></label>
        <label class="field"><span class="field-label">核心欲望</span><input v-model="core.desire" class="input" /></label>
      </div>
      <div style="display:grid;gap:16px;margin-top:16px">
        <label class="field"><span class="field-label">核心机制</span><textarea v-model="core.mechanism" class="textarea" /></label>
        <label class="field"><span class="field-label">主要冲突</span><textarea v-model="core.conflict" class="textarea" /></label>
        <label class="field"><span class="field-label">希望读者持续期待什么</span><textarea v-model="core.readerPromise" class="textarea" /></label>
        <label class="field"><span class="field-label">必须遵守的世界规则（每行一条）</span><textarea v-model="core.worldRules" class="textarea" /></label>
        <label class="field"><span class="field-label">开篇暂不揭示的内容（每行一条）</span><textarea v-model="core.forbiddenReveals" class="textarea" /></label>
        <label class="field"><span class="field-label">叙述风格</span><input v-model="core.style" class="input" /></label>
        <label class="field"><span class="field-label">可扩展方向（每行一条）</span><textarea v-model="core.expandableDirections" class="textarea" /></label>
      </div>
      <div class="banner info" style="margin-top:16px"><div><div class="banner-title">下一步：故事底稿与风格卡</div><div class="banner-copy">保存后先确定故事底稿和写法风格，再为第一章建立最小的正式规划。</div></div></div>
      <CommandFeedback :state="state" @confirm="store.confirmCommandResult('story-setup-save')" />
      <div class="actions" style="justify-content:flex-end;margin-top:16px"><button class="btn primary" :disabled="!core.protagonist.trim() || !core.readerPromise.trim() || ['submitting','accepted','processing'].includes(state.status)" @click="saveAndContinue">保存故事核心并继续</button></div>
    </section>
  </div>
</template>

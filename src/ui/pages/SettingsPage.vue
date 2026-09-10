<script setup lang="ts">
import { computed, onMounted, reactive, ref, watchEffect } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'
import type { SettingsProjection } from '../../application/contracts'

const route=useRoute()
const projectStore=useProjectStore()
const libraryStore=useProjectLibraryStore()
const isProjectSettings=computed(()=>route.meta.scope==='project')
const section=ref('model')
const form=reactive<SettingsProjection>({provider:'OpenAI Compatible',model:'gpt-4o-mini',endpoint:'',monthlyBudget:200,chapterBudget:3,pauseBeforeOverrun:true,privacyMode:'local-first',sendDraftOnly:true,exportFormat:'DOCX',platform:'番茄小说',theme:'light',providerId:'openai-compatible',modelId:'gpt-4o-mini',enabled:false,timeoutMs:30000})
const apiKey=ref('')

onMounted(()=>{ if(isProjectSettings.value) void projectStore.loadSettings(); else void libraryStore.loadGlobalSettings() })
watchEffect(()=>{
  if(!isProjectSettings.value && !libraryStore.globalSettings) void libraryStore.loadGlobalSettings()
  const source=isProjectSettings.value ? projectStore.settings : libraryStore.globalSettings
  if(source) Object.assign(form,source)
})

const saveState=computed(()=>isProjectSettings.value ? projectStore.commandState('settings-save') : libraryStore.commandState('global-settings-save'))
const testState=computed(()=>isProjectSettings.value ? projectStore.commandState('provider-test') : libraryStore.commandState('global-provider-test'))
const backupState=computed(()=>projectStore.commandState('project-backup'))
const integrityState=computed(()=>projectStore.commandState('project-integrity'))

function providerSettings(){
  const model=form.model.trim()
  return {...form,model,modelId:model,enabled:Boolean(form.endpoint.trim()),...(apiKey.value ? {apiKey:apiKey.value} : {})}
}
async function save(){
  const settings=providerSettings()
  if(isProjectSettings.value) await projectStore.runCommand('settings-save',{type:'settings_save',payload:{settings}})
  else { await libraryStore.runCommand('global-settings-save',{type:'global_settings_save',payload:{settings}}); libraryStore.globalSettings={...form,model:settings.model,modelId:settings.modelId,enabled:settings.enabled} }
  apiKey.value=''
}
async function test(){
  const settings=providerSettings()
  if(isProjectSettings.value) await projectStore.testProvider('provider-test',settings)
  else await libraryStore.testProvider('global-provider-test',settings)
}
async function backup(){ await projectStore.runCommand('project-backup',{type:'backup_create',payload:{kind:'milestone'}}) }
async function integrity(){ await projectStore.runCommand('project-integrity',{type:'integrity_check_run',payload:{mode:'quick_check'}}) }
function applyTheme(){ document.documentElement.dataset.theme=form.theme==='system'?(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):form.theme }
function scrollTo(id:string){ section.value=id; document.getElementById(`setting-${id}`)?.scrollIntoView({behavior:'smooth'}) }
function resetForm(){ const source=isProjectSettings.value ? projectStore.settings : libraryStore.globalSettings; if(source) Object.assign(form,source) }
</script>

<template>
  <div class="page">
    <PageHeader :title="isProjectSettings ? `项目设置 · ${projectStore.project?.title ?? ''}` : '应用设置'" :subtitle="isProjectSettings ? '这些设置只作用于当前作品；预算、目标平台和导出偏好不会污染其他作品。' : '设置模型服务、默认预算、隐私和外观；新作品可以继承后再独立修改。'">
      <button class="btn primary" @click="save">保存更改</button>
    </PageHeader>
    <CommandFeedback :state="saveState" />

    <div class="settings-layout section">
      <aside class="settings-nav"><button :class="{active:section==='model'}" @click="scrollTo('model')">模型配置</button><button :class="{active:section==='cost'}" @click="scrollTo('cost')">成本控制</button><button :class="{active:section==='privacy'}" @click="scrollTo('privacy')">隐私</button><button :class="{active:section==='export'}" @click="scrollTo('export')">导出</button><button v-if="isProjectSettings" :class="{active:section==='reliability'}" @click="scrollTo('reliability')">数据可靠性</button><button :class="{active:section==='platform'}" @click="scrollTo('platform')">平台偏好</button><button :class="{active:section==='appearance'}" @click="scrollTo('appearance')">外观</button></aside>

      <main>
        <section id="setting-model" class="setting-section">
          <div class="card-head"><div><h2 class="card-title">模型配置</h2><p class="card-subtitle">可在这里测试模型服务连接。应用设置作为默认配置，单个作品可以使用独立的模型设置。</p></div></div>
          <div class="setting-grid"><label class="field"><span class="field-label">服务类型</span><select v-model="form.provider" class="select"><option>OpenAI Compatible</option><option>自定义兼容服务</option></select></label><label class="field"><span class="field-label">默认创作模型</span><input v-model="form.model" class="input" /></label><label class="field" style="grid-column:1/-1"><span class="field-label">接口地址（API Endpoint）</span><input v-model="form.endpoint" class="input" /></label><label class="field" style="grid-column:1/-1"><span class="field-label">访问密钥（API Key）</span><input v-model="apiKey" class="input" type="password" autocomplete="new-password" /><span class="help">密钥会安全保存在系统凭据存储中，不会写入作品数据或操作记录。留空表示继续使用已保存的密钥。</span></label></div>
          <div class="actions" style="margin-top:14px"><button class="btn" @click="test">测试模型连接</button></div><CommandFeedback :state="testState" />
        </section>

        <section id="setting-cost" class="setting-section">
          <div class="card-head"><div><h2 class="card-title">成本控制</h2><p class="card-subtitle">{{ isProjectSettings ? '只统计并约束当前作品。' : '作为新作品默认预算；每部作品仍单独统计实际成本。' }}达到预算前先暂停并请求你的决定。</p></div></div>
          <div class="setting-grid"><label class="field"><span class="field-label">项目总预算（¥）</span><input v-model.number="form.monthlyBudget" type="number" min="0" class="input" /></label><label class="field"><span class="field-label">单章预算上限（¥）</span><input v-model.number="form.chapterBudget" type="number" min="0" step=".1" class="input" /></label></div>
          <div class="toggle-row"><div><strong>超预算前自动暂停</strong><div class="muted" style="font-size:11px">暂停后进入决策收件箱，由你决定提高预算或降低模型成本。</div></div><button class="toggle" :class="{on:form.pauseBeforeOverrun}" @click="form.pauseBeforeOverrun=!form.pauseBeforeOverrun" /></div>
        </section>

        <section id="setting-privacy" class="setting-section">
          <div class="card-head"><div><h2 class="card-title">隐私设置</h2><p class="card-subtitle">本地优先：作品数据保存在本机，并以本机记录为准。</p></div></div>
          <div class="setting-grid"><label class="field"><span class="field-label">隐私模式</span><select v-model="form.privacyMode" class="select"><option value="local-first">本地优先</option><option value="balanced">平衡模式</option></select></label></div>
          <div class="toggle-row"><div><strong>模型请求仅发送必要内容</strong><div class="muted" style="font-size:11px">系统只提供当前任务真正需要的作品信息。</div></div><button class="toggle" :class="{on:form.sendDraftOnly}" @click="form.sendDraftOnly=!form.sendDraftOnly" /></div>
        </section>

        <section id="setting-export" class="setting-section">
          <div class="card-head"><div><h2 class="card-title">导出设置</h2><p class="card-subtitle">默认只导出已经正式提交的正文。</p></div></div>
          <label class="field"><span class="field-label">默认格式</span><select v-model="form.exportFormat" class="select"><option>DOCX</option><option>Markdown</option><option>TXT</option><option>EPUB</option></select></label>
          <div class="banner info" style="margin-top:14px"><div><div class="banner-title">发布前人工确认</div><div class="banner-copy">系统负责完成作品生产、审核与正式成稿，但不会自动发布到外部平台；真正发布前由用户最终确认。</div></div></div>
        </section>

        <section v-if="isProjectSettings" id="setting-reliability" class="setting-section">
          <div class="card-head"><div><h2 class="card-title">数据可靠性</h2><p class="card-subtitle">备份和完整性检查会在后台完成；页面只提交操作并展示结果，不会直接改写作品正式记录。</p></div></div>
          <div class="actions"><button class="btn" @click="backup">创建里程碑备份</button><button class="btn" @click="integrity">运行完整性检查</button></div>
          <CommandFeedback :state="backupState" /><CommandFeedback :state="integrityState" />
          <div class="banner info" style="margin-top:12px"><div><div class="banner-title">可靠性基线</div><div class="banner-copy">正式实现包含本机事务、崩溃恢复、快速检查、哈希验证、引用检查和只读恢复模式。</div></div></div>
        </section>

        <section id="setting-platform" class="setting-section">
          <div class="card-head"><div><h2 class="card-title">平台偏好</h2><p class="card-subtitle">平台规则必须有版本、来源和采集时间；核心平台改变会使旧开篇评估回执失效。</p></div></div>
          <label class="field"><span class="field-label">默认目标平台</span><select v-model="form.platform" class="select"><option>AI 推荐</option><option>番茄小说</option><option>起点中文网</option><option>晋江文学城</option></select></label>
        </section>

        <section id="setting-appearance" class="setting-section">
          <div class="card-head"><div><h2 class="card-title">外观</h2><p class="card-subtitle">保持安静、编辑部式的阅读与制作体验。</p></div></div>
          <div class="segmented"><button :class="{active:form.theme==='light'}" @click="form.theme='light';applyTheme()">浅色</button><button :class="{active:form.theme==='system'}" @click="form.theme='system';applyTheme()">跟随系统</button><button :class="{active:form.theme==='dark'}" @click="form.theme='dark';applyTheme()">深色</button></div>
        </section>

        <div class="actions" style="justify-content:space-between"><button class="btn soft" @click="resetForm">恢复当前已保存设置</button><button class="btn primary" @click="save">保存更改</button></div>
      </main>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import BaseModal from '../components/BaseModal.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'
import { blockReasonLabels, phaseLabel, runStateLabel, signingLabels } from '../presentation/projectLabels'
import { productErrorMessage, productTimeLabel } from '../presentation/productLanguage'
import type { ProjectLibraryItem } from '../../application/contracts'

const router = useRouter()
const store = useProjectLibraryStore()
const query = ref('')
const statusFilter = ref<'all'|'working'|'waiting'|'completed'|'paused'>('all')
const sortMode = ref<'recent'|'updated'|'title'|'cost'>('recent')
const showArchived = ref(false)
const actionTarget = ref<ProjectLibraryItem | null>(null)
const actionMode = ref<'archive'|'delete'|'duplicate'|null>(null)
const deleteConfirm = ref('')

onMounted(() => store.bootstrap())

const filteredProjects = computed(() => {
  const needle = query.value.trim().toLowerCase()
  let items = store.activeProjects.filter(item => {
    if (needle && !`${item.title} ${item.subtitle} ${item.genre} ${item.platform}`.toLowerCase().includes(needle)) return false
    if (statusFilter.value === 'working' && item.runState !== 'ACTIVE') return false
    if (statusFilter.value === 'waiting' && item.pendingDecisions === 0) return false
    if (statusFilter.value === 'completed' && item.phase !== 'COMPLETED') return false
    if (statusFilter.value === 'paused' && item.runState !== 'PAUSED') return false
    return true
  })
  items = items.slice().sort((a,b) => {
    if (sortMode.value === 'title') return a.title.localeCompare(b.title, 'zh-CN')
    if (sortMode.value === 'cost') return b.totalCost - a.totalCost
    if (sortMode.value === 'updated') return b.updatedAt.localeCompare(a.updatedAt)
    return b.lastOpenedAt.localeCompare(a.lastOpenedAt)
  })
  return items
})

function blockLabel(reason: ProjectLibraryItem['blockReason']) { return reason ? blockReasonLabels[reason] : '' }
function readErrorLabel(error: unknown) { return productErrorMessage(error, '暂时无法打开这部作品，请重试。') }
function displayTitle(item: ProjectLibraryItem | null | undefined) {
  return item?.title?.trim() || '未命名作品'
}

function statusClass(item: ProjectLibraryItem) {
  if (item.runState === 'FAILED' || item.runState === 'BLOCKED') return 'danger'
  if (item.pendingDecisions) return 'warning'
  if (item.phase === 'COMPLETED') return 'success'
  if (item.runState === 'ACTIVE') return 'pine'
  return 'info'
}

function openProject(item: ProjectLibraryItem) {
  router.push(`/projects/${item.id}`)
}

function askAction(item: ProjectLibraryItem, mode: 'archive'|'delete'|'duplicate') {
  if (!item.operational && mode === 'duplicate') return
  actionTarget.value = item
  actionMode.value = mode
  deleteConfirm.value = ''
}

function closeAction() {
  actionTarget.value = null
  actionMode.value = null
  deleteConfirm.value = ''
}

async function duplicateProject() {
  const target = actionTarget.value
  if (!target || !target.operational) return
  const receipt = await store.runCommand(`duplicate-${target.id}`, { type:'project_duplicate', projectId:target.id, expectedRevision:target.revision })
  closeAction()
  if (receipt.finalStatus !== 'committed') return
  const createdProjectId = receipt.resultProjectId
  if (!createdProjectId) throw new Error('PROTOCOL / RESULT_PROJECT_ID_MISSING')
  router.push(`/projects/${createdProjectId}`)
}

async function archiveProject() {
  const target = actionTarget.value
  if (!target) return
  const request: { type: 'project_archive'; projectId: string; expectedRevision?: number } = { type:'project_archive', projectId:target.id }
  if (target.revision != null) request.expectedRevision = target.revision
  await store.runCommand(`archive-${target.id}`, request)
  if (store.commandState(`archive-${target.id}`).status === 'committed') closeAction()
}

async function restoreProject(item: ProjectLibraryItem) {
  if (!item.operational) return
  await store.runCommand(`restore-${item.id}`, { type:'project_restore', projectId:item.id, expectedRevision:item.revision })
}

async function deleteProject() {
  const target = actionTarget.value
  if (!target || deleteConfirm.value !== displayTitle(target)) return
  const request: { type: 'project_delete'; projectId: string; expectedRevision?: number } = { type:'project_delete', projectId:target.id }
  if (target.revision != null) request.expectedRevision = target.revision
  try {
    await store.runCommand(`delete-${target.id}`, request)
    if (store.commandState(`delete-${target.id}`).status === 'committed') closeAction()
  } catch {
    // The store owns the failed state; consume the event error so a failed
    // destructive action never escapes into the browser event loop.
  }
}
</script>

<template>
  <div class="page project-library-page">
    <PageHeader title="作品库" subtitle="每部小说都拥有独立的生产状态、正文、作品图谱、预算、审核记录和版本历史。切换作品不会混用上下文。">
      <button class="btn primary" @click="router.push('/projects/new')">＋ 新建作品</button>
    </PageHeader>

    <div v-if="store.loadError" class="banner danger"><div><div class="banner-title">无法读取作品库</div><div class="banner-copy">{{ productErrorMessage(store.loadError, '暂时无法读取作品库，请重试。') }}</div></div><button class="btn small" @click="store.refresh">重试</button></div>

    <template v-if="store.projection">
      <div class="metric-grid library-metrics">
        <MetricCard label="活跃作品" :value="store.projection.totals.active" note="不含已归档作品" />
        <MetricCard label="正在生产" :value="store.projection.totals.producing" note="当前有后台生产任务" />
        <MetricCard label="等待决定" :value="store.projection.totals.waitingDecision" note="需要你处理后才能继续" />
        <MetricCard label="累计成本" :value="`¥${store.projection.totals.totalCost.toFixed(2)}`" note="全部作品合计" />
      </div>

      <section class="library-toolbar card">
        <div class="library-search"><span>⌕</span><input v-model="query" placeholder="搜索作品名称、题材或平台…" /></div>
        <div class="segmented library-filter">
          <button :class="{active:statusFilter==='all'}" @click="statusFilter='all'">全部</button>
          <button :class="{active:statusFilter==='working'}" @click="statusFilter='working'">生产中</button>
          <button :class="{active:statusFilter==='waiting'}" @click="statusFilter='waiting'">待决定</button>
          <button :class="{active:statusFilter==='paused'}" @click="statusFilter='paused'">已暂停</button>
          <button :class="{active:statusFilter==='completed'}" @click="statusFilter='completed'">已完成</button>
        </div>
        <select v-model="sortMode" class="select library-sort" aria-label="作品排序">
          <option value="recent">最近打开</option>
          <option value="updated">最近更新</option>
          <option value="title">作品名称</option>
          <option value="cost">累计成本</option>
        </select>
      </section>

      <section class="section">
        <div class="section-head"><div><h2>我的作品</h2><div class="muted" style="font-size:12px">{{ filteredProjects.length }} 部匹配当前筛选</div></div></div>
        <div v-if="filteredProjects.length" class="project-grid">
          <article v-for="item in filteredProjects" :key="item.id" class="project-card">
            <button class="project-card-main" @click="openProject(item)">
              <div class="project-cover" :class="`cover-${Math.abs(displayTitle(item).charCodeAt(0))%4}`"><span>{{ displayTitle(item).slice(0,1) }}</span><small>{{ item.genre.split('·')[0].trim() }}</small></div>
              <div class="project-card-copy">
                <div class="project-card-title-row"><h3>{{ displayTitle(item) }}</h3><span class="badge" :class="statusClass(item)">{{ item.pendingDecisions ? `待决定 ${item.pendingDecisions}` : runStateLabel(item.runState) }}</span></div>
                <p>{{ item.subtitle }}</p>
                <div class="project-tags"><span>{{ item.platform }}</span><span>{{ item.genre }}</span><span>{{ item.mode==='AUTO'?'全自动':'导演模式' }}</span></div>
                <div class="project-progress-row"><div class="progress"><span :style="{width:`${item.progress}%`}" /></div><b>{{ item.progress }}%</b></div>
                <div class="project-stats"><span><b>{{ item.committedChapters }}</b> 正式章节</span><span><b>{{ item.totalWords.toLocaleString() }}</b> 字</span><span><b>¥{{ item.totalCost.toFixed(2) }}</b></span><span>{{ phaseLabel(item.phase) }}</span></div>
                <div v-if="!item.operational" class="project-next warning-text">暂时无法打开：{{ readErrorLabel(item.readError) }}</div><div v-else-if="item.blockReason" class="project-next warning-text">当前原因：{{ blockLabel(item.blockReason) }}</div>
                <div v-else class="project-next">开篇评估：{{ signingLabels[item.signingStatus] }} · 最近打开 {{ productTimeLabel(item.lastOpenedAt) }}</div>
              </div>
            </button>
            <div class="project-card-actions">
              <button class="btn small primary" @click="openProject(item)">{{ item.pendingDecisions ? '处理并继续' : item.phase==='COMPLETED' ? '打开作品' : '继续创作' }}</button>
              <button class="btn small soft" :disabled="!item.operational" @click="askAction(item,'duplicate')">复制</button>
              <button class="btn small soft" @click="askAction(item,'archive')">归档</button>
              <button class="btn small ghost danger-text" @click="askAction(item,'delete')">删除</button>
            </div>
            <CommandFeedback :state="store.commandState(`archive-${item.id}`)" />
          </article>
        </div>
        <div v-else class="empty card"><div class="empty-symbol">⌕</div><strong>没有匹配的作品</strong><p>换一个筛选条件，或者新建一部作品。</p><button class="btn primary" @click="router.push('/projects/new')">新建作品</button></div>
      </section>

      <section class="section archived-section">
        <button class="archived-toggle" @click="showArchived=!showArchived"><span><strong>已归档作品</strong><small>{{ store.archivedProjects.length }} 部 · 归档不会删除正文或版本</small></span><span>{{ showArchived ? '收起' : '展开' }}⌄</span></button>
        <div v-if="showArchived" class="archived-list card">
          <div v-for="item in store.archivedProjects" :key="item.id" class="archived-row">
            <div><strong>{{ displayTitle(item) }}</strong><div class="muted" style="font-size:11px">{{ item.genre }} · 最后更新 {{ productTimeLabel(item.updatedAt) }}</div></div>
            <div class="actions"><button class="btn small" :disabled="!item.operational" @click="restoreProject(item)">恢复到作品库</button><button class="btn small ghost danger-text" @click="askAction(item,'delete')">删除</button></div>
          </div>
          <div v-if="!store.archivedProjects.length" class="empty">暂无归档作品</div>
        </div>
      </section>
    </template>

    <BaseModal :open="!!actionTarget && actionMode==='duplicate'" title="复制作品" :subtitle="actionTarget ? `将创建《${displayTitle(actionTarget)}》的独立副本。` : ''" @close="closeAction">
      <div class="banner info"><div><div class="banner-title">副本会作为独立作品创建</div><div class="banner-copy">正文、图谱和版本会复制，但运行状态会重置为“未运行”，不会继续原作品正在执行的任务。</div></div></div>
      <CommandFeedback v-if="actionTarget" :state="store.commandState(`duplicate-${actionTarget.id}`)" />
      <template #footer><button class="btn" @click="closeAction">取消</button><button class="btn primary" @click="duplicateProject">创建副本</button></template>
    </BaseModal>

    <BaseModal :open="!!actionTarget && actionMode==='archive'" title="归档作品" :subtitle="actionTarget ? `归档《${displayTitle(actionTarget)}》后，它会从默认作品列表隐藏。` : ''" @close="closeAction">
      <div class="banner warning"><div><div class="banner-title">归档不是删除</div><div class="banner-copy">正式章节、作品图谱、候选稿、模型回执和版本历史都会保留；正在运行的作品必须先暂停。</div></div></div>
      <CommandFeedback v-if="actionTarget" :state="store.commandState(`archive-${actionTarget.id}`)" />
      <template #footer><button class="btn" @click="closeAction">取消</button><button class="btn warning" @click="archiveProject">确认归档</button></template>
    </BaseModal>

    <BaseModal :open="!!actionTarget && actionMode==='delete'" title="永久删除作品" :subtitle="actionTarget ? `删除《${displayTitle(actionTarget)}》是不可逆操作。` : ''" @close="closeAction">
      <div class="banner danger"><div><div class="banner-title">删除前请确认</div><div class="banner-copy">删除作品时会同时清理与它关联的临时内容和缓存，并保留必要的安全操作记录。</div></div></div>
      <label v-if="actionTarget" class="field" style="margin-top:16px"><span class="field-label">输入作品名“{{ displayTitle(actionTarget) }}”确认删除</span><input v-model="deleteConfirm" class="input" /></label>
      <CommandFeedback v-if="actionTarget" :state="store.commandState(`delete-${actionTarget.id}`)" />
      <template #footer><button class="btn" @click="closeAction">取消</button><button class="btn danger" :disabled="!actionTarget || deleteConfirm!==displayTitle(actionTarget)" @click="deleteProject">永久删除</button></template>
    </BaseModal>
  </div>
</template>

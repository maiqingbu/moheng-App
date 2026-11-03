/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'

const harness = vi.hoisted(() => ({ store: null as any }))
vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'project-craft' }, query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}))
vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))

const bible = (index: number) => ({
  id: `story-bible:p:c:${index}`, stage: 'CANDIDATE', generation: 1, variant: `MODEL_${index}`,
  angle: `侧重角度${index}`,
  normalWorld: '林满在码头记账。',
  mechanism: { experience: '听旧物的声音', rules: [{ rule: '听一次忘一天', numbers: '每日三次', howReadersSeeIt: '想不起账' }, { rule: '一年为限', numbers: '一年', howReadersSeeIt: '旧表无声' }] },
  protagonist: { name: `林满${index}`, personality: '嘴硬心软', flaw: '先算代价', wantsNow: '赎回面馆', humorStyle: '抠字眼', innerVoiceStyle: '像记账' },
  emotionalAnchor: { object: '围裙', memorySummary: '发烧那夜' },
  antagonist: { name: '彭三', position: '放贷二当家', wants: '吞地', method: '借据' },
  cast: [
    { name: '阿秋', role: '伙计', relation: '发小', privateAgenda: '学厨', speechStyle: '话少', contrastWithProtagonist: '冲动', firstAppears: 1 },
    { name: '老董', role: '船老大', relation: '主顾', privateAgenda: '欠钱', speechStyle: '俗语', contrastWithProtagonist: '认命', firstAppears: 2 },
  ],
  openingArc: [1, 2, 3].map(chapter => ({ chapter, direction: `走向${chapter}`, protagonistWin: `赢${chapter}` })),
})
const range = (min: number, max: number) => ({ min, max })
const card = (index: number) => ({
  id: `style-card:p:c:${index}`, stage: 'CANDIDATE', generation: 1, variant: `A2_V${index}`, label: `冷峻市井版${index}`,
  source: { kind: 'MARKET_ARCHETYPE', archetypeIds: ['A2'], sampleDigest: null },
  targets: { avgParagraphChars: range(23, 30), shortParagraphPct: range(35, 50), dialogueParagraphPct: range(35, 65), similePer1k: range(0, 1), explainPer1k: range(0, 1.5), exclaimPer1k: range(0, 3), questionPer1k: range(2, 6), ellipsisPer1k: range(2, 10) },
  narration: { person: 'THIRD', povPolicy: 'STRICT', voice: '话少' },
  openingStructure: ['地点三连', '一笔交易', '主角的账本'],
  rules: ['用钱标处境', '每场一个地名', '短句列选项', '暴力突然且短'],
  forbidden: ['抒情环境描写'],
  examples: [],
})

const mounted: Array<ReturnType<typeof createApp>> = []
afterEach(() => {
  mounted.splice(0).forEach(app => app.unmount())
  document.body.innerHTML = ''
})

async function flush() {
  for (let index = 0; index < 10; index++) await Promise.resolve()
  await nextTick()
}

async function mountPage() {
  harness.store = reactive({
    currentProjectId: 'project-craft',
    craftSetup: {
      storyBible: { candidates: [1, 2, 3].map(bible), confirmed: null },
      styleCard: { candidates: [1, 2, 3].map(card), confirmed: null },
      sample: null,
      archetypes: [{ id: 'A2', name: '冷峻市井', genres: ['年代'] }, { id: 'A3', name: '悬疑惊悚', genres: ['悬疑'] }],
    },
    loadCraftSetup: vi.fn(async () => harness.store.craftSetup),
    commandState: () => ({ status: 'idle', message: '' }),
    confirmCommandResult: vi.fn(),
    runCommand: vi.fn(async () => ({ finalStatus: 'committed' })),
  })
  const { default: CraftSetup } = await import('../../src/ui/pages/CraftSetup.vue')
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(CraftSetup)
  mounted.push(app)
  app.mount(root)
  await flush()
  return root
}

const button = (root: HTMLElement, text: string, nth = 0) =>
  [...root.querySelectorAll('button')].filter(item => item.textContent?.includes(text))[nth] as HTMLButtonElement

async function type(input: HTMLInputElement | HTMLTextAreaElement, value: string) {
  input.value = value
  input.dispatchEvent(new Event('input'))
  await flush()
}

describe('故事底稿与风格卡页面', () => {
  it('Given 3 版底稿候选 When 选第 2 版并改对手姓名 Then 确认命令带候选编号与修改后的内容', async () => {
    const root = await mountPage()
    expect(root.textContent).toContain('侧重角度1')
    expect(root.textContent).toContain('第 3 章赢：赢3')
    expect(root.textContent).not.toMatch(/story-bible:|MODEL_/)

    button(root, '选这一版并编辑', 1).click()
    await flush()
    const antagonistName = [...root.querySelectorAll('label')].find(label => label.textContent?.includes('手段'))
      ?.parentElement?.querySelector('input') as HTMLInputElement
    await type(antagonistName, '彭老三')
    button(root, '确认这版底稿').click()
    await flush()

    const [key, request] = harness.store.runCommand.mock.calls.at(-1)
    expect(key).toBe('story-bible-confirm')
    expect(request.type).toBe('story_bible_confirm')
    expect(request.payload.candidateId).toBe('story-bible:p:c:2')
    expect(request.payload.bible.antagonist.name).toBe('彭老三')
    expect(request.payload.bible).not.toHaveProperty('id')
  })

  it('Given 编辑底稿 When 在内心声音里写了带引号的台词 Then 显示原因且不能确认', async () => {
    const root = await mountPage()
    button(root, '选这一版并编辑').click()
    await flush()
    const voice = [...root.querySelectorAll('label')].find(label => label.textContent?.includes('内心声音'))?.querySelector('input') as HTMLInputElement
    await type(voice, '常说“我记下了”')
    expect(root.textContent).toContain('不要写带引号的台词')
    expect(button(root, '确认这版底稿').disabled).toBe(true)
  })

  it('Given 3 版风格卡 When 选一版并增加规则 Then 确认命令把多行规则拆成列表', async () => {
    const root = await mountPage()
    expect(root.textContent).toContain('冷峻市井版1')
    expect(root.textContent).toContain('第三人称 · 只跟主角视角 · 话少')
    button(root, '选这一版并编辑', 3).click()
    await flush()
    const rules = [...root.querySelectorAll('label')].find(label => label.textContent?.includes('写法规则'))?.querySelector('textarea') as HTMLTextAreaElement
    await type(rules, `${rules.value}\n每章至少一处钱数变化`)
    button(root, '确认这版风格卡').click()
    await flush()
    const [key, request] = harness.store.runCommand.mock.calls.at(-1)
    expect(key).toBe('style-card-confirm')
    expect(request.payload.candidateId).toBe('style-card:p:c:1')
    expect(request.payload.card.rules).toHaveLength(5)
    expect(request.payload.card.rules.at(-1)).toBe('每章至少一处钱数变化')
  })

  it('Given 选择我的范文 When 范文不足 800 字 Then 提示差多少字且不能保存，并说明只保存在本机', async () => {
    const root = await mountPage()
    const sampleRadio = root.querySelector('input[type=radio][value=USER_SAMPLE]') as HTMLInputElement
    sampleRadio.checked = true
    sampleRadio.dispatchEvent(new Event('change'))
    await flush()
    const textarea = [...root.querySelectorAll('label')].find(label => label.textContent?.includes('范文（'))?.querySelector('textarea') as HTMLTextAreaElement
    await type(textarea, '码头的灯灭了。')
    expect(root.textContent).toContain('还差')
    expect(root.textContent).toContain('只保存在本机')
    expect(button(root, '保存范文').disabled).toBe(true)
    expect(button(root, '生成 3 版风格卡').disabled).toBe(true)
  })
})

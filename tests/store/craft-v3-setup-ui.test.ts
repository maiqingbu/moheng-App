/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  bibleProblems,
  bibleSummary,
  craftConfirmRequest,
  linesToList,
  listToLines,
  sampleReadiness,
  storyBibleGenerateRequest,
  styleCardGenerateRequest,
  styleSummary,
  TARGET_LABELS,
} from '../../src/ui/presentation/craftSetup'
import { productErrorMessage } from '../../src/ui/presentation/productLanguage'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')

const bible = () => ({
  id: 'story-bible:p:c:1', payloadSchema: 'story-bible.r1', stage: 'CANDIDATE', generation: 1, variant: 'MODEL_1',
  angle: '侧重市井人情',
  normalWorld: '林满在码头记账。',
  mechanism: { experience: '听旧物的声音', rules: [{ rule: '听一次忘一天', numbers: '每日三次', howReadersSeeIt: '想不起账' }, { rule: '一年为限', numbers: '一年', howReadersSeeIt: '旧表无声' }] },
  protagonist: { name: '林满', personality: '嘴硬心软', flaw: '先算代价', wantsNow: '赎回面馆', humorStyle: '抠字眼', innerVoiceStyle: '像记账' },
  emotionalAnchor: { object: '围裙', memorySummary: '发烧那夜' },
  antagonist: { name: '彭三', position: '放贷二当家', wants: '吞地', method: '借据' },
  cast: [
    { name: '阿秋', role: '伙计', relation: '发小', privateAgenda: '学厨', speechStyle: '话少', contrastWithProtagonist: '冲动', firstAppears: 1 },
    { name: '老董', role: '船老大', relation: '主顾', privateAgenda: '欠钱', speechStyle: '俗语', contrastWithProtagonist: '认命', firstAppears: 2 },
  ],
  openingArc: [1, 2, 3].map(chapter => ({ chapter, direction: `走向${chapter}`, protagonistWin: `赢${chapter}` })),
})

describe('故事底稿与风格卡（CRAFT v3 第 1 步）', () => {
  it('Given 底稿候选 When 并排对比 Then 摘要给出角度、主角、对手和三章各赢一局', () => {
    const summary = bibleSummary(bible())
    expect(summary.title).toBe('侧重市井人情')
    expect(summary.lines).toContain('主角：林满——赎回面馆')
    expect(summary.lines).toContain('对手：彭三（放贷二当家）')
    expect(summary.lines).toContain('第 2 章赢：赢2')
  })

  it('Given 用户编辑底稿 When 检查 Then 缺项、章数不对或风格里写了引号台词都会提示，合格时无提示', () => {
    expect(bibleProblems(bible())).toEqual([])
    const broken = bible()
    broken.openingArc.pop()
    broken.protagonist.innerVoiceStyle = '常说“我记下了”'
    broken.antagonist.name = ' '
    const problems = bibleProblems(broken)
    expect(problems).toContain('开篇走向需要正好 3 章')
    expect(problems).toContain('主角内心声音只写风格描述，不要写带引号的台词')
    expect(problems).toContain('对手姓名不能为空')
  })

  it('Given 生成与确认 When 组装命令 Then 底稿带调整意见，风格卡带来源与原型，确认时去掉候选编号字段', () => {
    expect(storyBibleGenerateRequest(' 多些人情味 ')).toEqual({ type: 'story_bible_generate', payload: { adjustment: '多些人情味' } })
    expect(styleCardGenerateRequest('MARKET_ARCHETYPE', 'A3', 'A6')).toEqual({ type: 'style_card_generate', payload: { source: 'MARKET_ARCHETYPE', primary: 'A3', secondary: 'A6' } })
    expect(styleCardGenerateRequest('USER_SAMPLE')).toEqual({ type: 'style_card_generate', payload: { source: 'USER_SAMPLE' } })
    const request = craftConfirmRequest('bible', 'story-bible:p:c:1', bible())
    expect(request.type).toBe('story_bible_confirm')
    expect(request.payload.candidateId).toBe('story-bible:p:c:1')
    expect(request.payload.bible).not.toHaveProperty('id')
    expect(craftConfirmRequest('card', 'style-card:p:c:2', { id: 'x', rules: [] }).payload).toEqual({ candidateId: 'style-card:p:c:2', card: { rules: [] } })
  })

  it('Given 风格卡 When 展示 Then 指标用中文名称，摘要给出叙述人称与前三条规则', () => {
    expect(TARGET_LABELS.avgParagraphChars).toBe('平均段长（字）')
    expect(Object.keys(TARGET_LABELS)).toHaveLength(8)
    const summary = styleSummary({ label: '冷峻市井（纯版）', narration: { person: 'THIRD', povPolicy: 'STRICT', voice: '话少' }, rules: ['甲', '乙', '丙', '丁'] })
    expect(summary.title).toBe('冷峻市井（纯版）')
    expect(summary.lines).toEqual(['第三人称 · 只跟主角视角 · 话少', '甲', '乙', '丙'])
  })

  it('Given 范文 When 判断 Then 不足最低字数时提示还差多少，列表与多行文本可互转', () => {
    expect(sampleReadiness('一二三', 800)).toEqual({ chars: 3, enough: false, message: '还差 797 字才能测量写法' })
    expect(sampleReadiness('字'.repeat(800), 800).enough).toBe(true)
    expect(linesToList(' 甲 \n\n乙\n')).toEqual(['甲', '乙'])
    expect(listToLines(['甲', '乙'])).toBe('甲\n乙')
  })

  it('Given 新错误码 When 展示 Then 给出中文说明', () => {
    expect(productErrorMessage('STORY_BIBLE_READY_SENTENCE:protagonist.innerVoiceStyle')).toContain('台词')
    expect(productErrorMessage('STYLE_SAMPLE_TOO_SHORT')).toContain('范文')
    expect(productErrorMessage('STYLE_SAMPLE_MISSING')).toContain('范文')
    expect(productErrorMessage('B11_STORY_SETUP_REQUIRED')).toContain('故事核心')
  })

  it('Given 页面与流程 When 检查 Then 故事核心之后进入底稿与风格卡页，再进入开篇准备', () => {
    const page = source('src/ui/pages/CraftSetup.vue')
    expect(page).toContain('故事底稿')
    expect(page).toContain('风格卡')
    expect(page).toContain('storyBibleGenerateRequest(')
    expect(page).toContain('craftConfirmRequest(')
    expect(page).toContain("projectPath('opening')")
    expect(page).toContain('只保存在本机')
    expect(source('src/ui/pages/StorySetup.vue')).toContain('/craft`')
    expect(source('src/app/router.ts')).toContain("path: '/projects/:id/craft'")
    const contracts = source('src/application/contracts.ts')
    for (const type of ['story_bible_generate', 'story_bible_confirm', 'style_card_generate', 'style_card_confirm', 'style_sample_save']) {
      expect(contracts).toContain(`| '${type}'`)
    }
    expect(contracts).toContain('queryCraftSetup(')
    expect(source('src/infrastructure/tauri/frontendPort.ts')).toContain("'query_craft_setup'")
    expect(source('scripts/verify-product-ux.mjs')).toContain("'src/ui/pages/CraftSetup.vue'")
  })
})

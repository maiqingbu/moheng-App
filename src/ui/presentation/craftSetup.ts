// CRAFT v3 step 1: story bible and style card presentation helpers
// (docs/development/CRAFT_V3_DESIGN.md §4.1).

type Json = Record<string, any>

export const TARGET_LABELS: Record<string, string> = {
  avgParagraphChars: '平均段长（字）',
  shortParagraphPct: '短段占比（%）',
  dialogueParagraphPct: '对白段占比（%）',
  similePer1k: '比喻（每千字）',
  explainPer1k: '因果解释（每千字）',
  exclaimPer1k: '感叹号（每千字）',
  questionPer1k: '问号（每千字）',
  ellipsisPer1k: '省略号（每千字）',
}

const QUOTE_MARKS = /[“”「」"]/
const text = (value: unknown) => (typeof value === 'string' ? value.trim() : '')

export function linesToList(value: string): string[] {
  return value.split('\n').map(line => line.trim()).filter(Boolean)
}

export function listToLines(value: unknown): string {
  return Array.isArray(value) ? value.filter(item => typeof item === 'string').join('\n') : ''
}

export function bibleSummary(bible: Json): { title: string; lines: string[] } {
  return {
    title: text(bible.angle) || '故事底稿',
    lines: [
      `主角：${text(bible.protagonist?.name)}——${text(bible.protagonist?.wantsNow)}`,
      `对手：${text(bible.antagonist?.name)}（${text(bible.antagonist?.position)}）`,
      ...(Array.isArray(bible.openingArc) ? bible.openingArc : []).map(
        (arc: Json) => `第 ${arc.chapter} 章赢：${text(arc.protagonistWin)}`,
      ),
    ],
  }
}

/** Client-side hints that mirror the server checks, so the user sees why
 *  confirming would be refused before sending. */
export function bibleProblems(bible: Json): string[] {
  const problems: string[] = []
  const required: Array<[unknown, string]> = [
    [bible.normalWorld, '日常世界'],
    [bible.mechanism?.experience, '能力体验'],
    [bible.protagonist?.name, '主角姓名'],
    [bible.protagonist?.wantsNow, '主角眼下想要'],
    [bible.antagonist?.name, '对手姓名'],
    [bible.emotionalAnchor?.object, '情感锚点物件'],
  ]
  for (const [value, label] of required) if (!text(value)) problems.push(`${label}不能为空`)
  const rules = bible.mechanism?.rules ?? []
  if (rules.length < 2 || rules.length > 5) problems.push('规则需要 2 到 5 条')
  const cast = bible.cast ?? []
  if (cast.length < 2 || cast.length > 8) problems.push('配角需要 2 到 8 人')
  if ((bible.openingArc ?? []).length !== 3) problems.push('开篇走向需要正好 3 章')
  if (QUOTE_MARKS.test(text(bible.protagonist?.innerVoiceStyle))) problems.push('主角内心声音只写风格描述，不要写带引号的台词')
  if (QUOTE_MARKS.test(text(bible.protagonist?.humorStyle))) problems.push('主角幽默方式只写风格描述，不要写带引号的台词')
  for (const member of cast) {
    if (QUOTE_MARKS.test(text(member.speechStyle))) problems.push(`${text(member.name) || '配角'}的说话方式只写风格描述，不要写带引号的台词`)
  }
  return problems
}

const PERSON: Record<string, string> = { FIRST: '第一人称', THIRD: '第三人称' }
const POV: Record<string, string> = { STRICT: '只跟主角视角', SHORT_CUTS_ALLOWED: '允许短切到对手' }

export function styleSummary(card: Json): { title: string; lines: string[] } {
  const narration = card.narration ?? {}
  return {
    title: text(card.label) || '风格卡',
    lines: [
      [PERSON[narration.person], POV[narration.povPolicy], text(narration.voice)].filter(Boolean).join(' · '),
      ...(Array.isArray(card.rules) ? card.rules.slice(0, 3) : []),
    ],
  }
}

export function sampleReadiness(sample: string, minChars: number): { chars: number; enough: boolean; message: string } {
  let chars = 0
  for (const char of sample) if (!/\s/.test(char)) chars += 1
  const enough = chars >= minChars
  return { chars, enough, message: enough ? `共 ${chars} 字，可以测量写法` : `还差 ${minChars - chars} 字才能测量写法` }
}

export function storyBibleGenerateRequest(adjustment: string) {
  return { type: 'story_bible_generate' as const, payload: { adjustment: adjustment.trim() } }
}

export function styleCardGenerateRequest(source: 'MARKET_ARCHETYPE' | 'USER_SAMPLE', primary?: string, secondary?: string) {
  const payload: Json = { source }
  if (primary) payload.primary = primary
  if (secondary) payload.secondary = secondary
  return { type: 'style_card_generate' as const, payload }
}

export function craftConfirmRequest(kind: 'bible' | 'card', candidateId: string, edited: Json) {
  const { id: _id, ...content } = edited
  return {
    type: kind === 'bible' ? ('story_bible_confirm' as const) : ('style_card_confirm' as const),
    payload: { candidateId, [kind]: content } as Json,
  }
}

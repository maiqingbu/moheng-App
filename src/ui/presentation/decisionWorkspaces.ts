import type { DecisionCategory } from '../../application/contracts'

/** Where in a project each kind of decision is made, and the button that goes there. */
export const decisionWorkspaces: Record<DecisionCategory, { path: string; label: string }> = {
  立项: { path: 'directions', label: '去故事页确认' },
  样章反馈: { path: 'production', label: '去写作页处理' },
  'AI 推断确认': { path: 'directions', label: '去故事页确认' },
  BookPlan: { path: 'production', label: '去写作页确认总纲' },
  卷方向: { path: 'volumes', label: '去故事页确认本卷' },
  预算: { path: 'settings', label: '去作品设置调整预算' },
  设定冲突: { path: 'health', label: '去资料库处理' },
  模拟签约: { path: 'production', label: '去写作页确认签约' },
  全局修改: { path: 'history', label: '去版本历史处理' },
  Retcon: { path: 'history', label: '去版本历史处理' },
  大规模修复: { path: 'production', label: '去写作页授权修复' },
  完结冲突: { path: 'completion', label: '去完结准备处理' },
  审计提醒: { path: 'read', label: '去稿件页阅读本章' },
  计划提醒: { path: 'production', label: '去写作页查看' },
  规划提醒: { path: 'volumes', label: '去故事页查看本卷' }
}

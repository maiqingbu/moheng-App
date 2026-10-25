import type { DecisionCategory } from '../../application/contracts'

/** Where in a project each kind of decision is made, and the button that goes there. */
export const decisionWorkspaces: Record<DecisionCategory, { path: string; label: string }> = {
  立项: { path: 'directions', label: '去规划确认' },
  样章反馈: { path: 'production', label: '去创作页处理' },
  'AI 推断确认': { path: 'directions', label: '去规划确认' },
  BookPlan: { path: 'production', label: '去创作页确认总纲' },
  卷方向: { path: 'volumes', label: '去当前卷确认' },
  预算: { path: 'settings', label: '去作品设置调整预算' },
  设定冲突: { path: 'health', label: '去长篇健康处理' },
  模拟签约: { path: 'production', label: '去创作页确认签约' },
  全局修改: { path: 'history', label: '去版本历史处理' },
  Retcon: { path: 'history', label: '去版本历史处理' },
  大规模修复: { path: 'production', label: '去创作页授权修复' },
  完结冲突: { path: 'completion', label: '去完结准备处理' }
}

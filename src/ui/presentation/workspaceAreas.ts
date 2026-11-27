// 2026-11-27 (owner approved the redesign): a work's pages in five areas.
// The left navigation, the tabs above a page and the assistant's sense of
// where the owner is all read this one list.

export interface AreaTab {
  label: string
  /** The route name. */
  name: string
  /** The path under /projects/:id. */
  path: string
}

export interface WorkspaceArea {
  label: string
  icon: string
  tabs: AreaTab[]
}

export const WORKSPACE_AREAS: WorkspaceArea[] = [
  { label: '首页', icon: '⌂', tabs: [{ label: '首页', name: 'project-overview', path: '' }, { label: '待决定', name: 'decisions', path: '/decisions' }] },
  { label: '写作', icon: '✎', tabs: [{ label: '写作', name: 'production', path: '/production' }, { label: '开篇三章', name: 'opening-ready', path: '/opening' }] },
  { label: '故事', icon: '◇', tabs: [{ label: '故事方向', name: 'directions', path: '/directions' }, { label: '市场定位', name: 'market', path: '/market' }, { label: '故事设定', name: 'story-setup', path: '/setup' }, { label: '写作风格', name: 'craft-setup', path: '/craft' }, { label: '分卷', name: 'volumes', path: '/volumes' }, { label: '完结准备', name: 'completion', path: '/completion' }] },
  // 版本与变更记录 (match: 'history') belongs with the manuscript.
  { label: '稿件', icon: '▤', tabs: [{ label: '阅读', name: 'read', path: '/read' }, { label: '版本记录', name: 'history', path: '/history' }] },
  // 长篇健康 (/health) belongs with the work's records.
  { label: '资料库', icon: '◎', tabs: [{ label: '作品图谱', name: 'graph', path: '/graph' }, { label: '长篇健康', name: 'health', path: '/health' }] },
]

/** The tab a route belongs to: one volume's workspace counts as 分卷. */
export function tabName(routeName: unknown): string {
  return routeName === 'volume-workspace' ? 'volumes' : String(routeName)
}

/** The area and tab of a route, when it is one of a work's pages. */
export function areaOf(routeName: unknown): { area: WorkspaceArea; tab: AreaTab } | null {
  const name = tabName(routeName)
  for (const area of WORKSPACE_AREAS) {
    const tab = area.tabs.find(item => item.name === name)
    if (tab) return { area, tab }
  }
  return null
}

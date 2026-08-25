import type { FrontendPort } from '../../application/contracts'
import { TauriFrontendPort, getTauriInvoke } from '../../infrastructure/tauri/frontendPort'
import { frontendPort as browserDemoPort } from '../../infrastructure/browser-demo/demoPort'

export function getFrontendPort(): FrontendPort {
  const invoke = getTauriInvoke()
  if (invoke) return new TauriFrontendPort(invoke)
  if (import.meta.env.VITE_BROWSER_DEMO === 'true') return browserDemoPort
  throw new Error('正式运行未配置 Tauri FrontendPort；已拒绝回退到浏览器 Demo 状态。')
}

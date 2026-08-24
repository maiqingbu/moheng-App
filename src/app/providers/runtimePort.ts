import type { FrontendPort } from '../../application/contracts'
import { frontendPort as browserDemoPort } from '../../infrastructure/browser-demo/demoPort'

/**
 * Composition root for the UI boundary.
 * BrowserDemoPort is allowed only in Vite dev or an explicitly opted-in demo build.
 * A production desktop build must provide the Tauri adapter before this boundary is used.
 */
export function getFrontendPort(): FrontendPort {
  const demoEnabled = import.meta.env.DEV || import.meta.env.VITE_BROWSER_DEMO === 'true'
  if (demoEnabled) return browserDemoPort
  throw new Error('正式运行未配置 Tauri FrontendPort；已拒绝回退到浏览器 Demo 状态。')
}

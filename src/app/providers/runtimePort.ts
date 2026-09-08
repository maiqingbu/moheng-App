import type { FrontendPort } from '../../application/contracts'
import { TauriFrontendPort, getTauriInvoke } from '../../infrastructure/tauri/frontendPort'
import { frontendPort as browserDemoPort } from '../../infrastructure/browser-demo/demoPort'

function getR4ProductionE2eInvoke() {
  if (import.meta.env.VITE_R4_PRODUCTION_E2E !== 'true') return null
  const endpoint = import.meta.env.VITE_R4_BRIDGE_URL
  const token = import.meta.env.VITE_R4_BRIDGE_TOKEN
  if (!endpoint || !token) throw new Error('R4_PRODUCTION_E2E_BRIDGE_CONFIG_MISSING')
  return {
    async invoke<T>(command: string, payload?: unknown): Promise<T> {
      const response = await fetch(endpoint, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-r4-bridge-token': token },
        body: JSON.stringify({ command, payload }),
      })
      const body = await response.json() as { error?: string } & T
      if (!response.ok || body.error) throw new Error(body.error ?? `R4_PRODUCTION_E2E_HTTP_${response.status}`)
      return body as T
    },
  }
}

export function getFrontendPort(): FrontendPort {
  const invoke = getTauriInvoke()
  if (invoke) return new TauriFrontendPort(invoke)
  const productionE2eInvoke = getR4ProductionE2eInvoke()
  if (productionE2eInvoke) return new TauriFrontendPort(productionE2eInvoke)
  if (import.meta.env.VITE_BROWSER_DEMO === 'true') return browserDemoPort
  throw new Error('正式运行未配置 Tauri FrontendPort；已拒绝回退到浏览器 Demo 状态。')
}

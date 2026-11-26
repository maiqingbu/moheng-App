// Owner decision 2026-11-27: common model services built in. Choosing one
// fills its OpenAI-compatible endpoint and suggests its usual models; the
// full list is still read from the service, and any name can be typed.

export interface ProviderPreset {
  id: string
  label: string
  endpoint: string
  /** Hosts (or host suffixes) that belong to this service. */
  hosts: string[]
  /** Usual models, shown first; the service's own list follows. */
  models: string[]
}

export const PROVIDER_PRESETS: ProviderPreset[] = [
  { id: 'bailian', label: '阿里云百炼', endpoint: 'https://dashscope.aliyuncs.com/compatible-mode/v1', hosts: ['aliyuncs.com'], models: ['qwen-plus', 'qwen-max', 'qwen-turbo', 'qwen3.8-flash', 'deepseek-v4.1-flash'] },
  { id: 'deepseek', label: 'DeepSeek', endpoint: 'https://api.deepseek.com/v1', hosts: ['deepseek.com'], models: ['deepseek-chat', 'deepseek-reasoner'] },
  { id: 'moonshot', label: 'Kimi（月之暗面）', endpoint: 'https://api.moonshot.cn/v1', hosts: ['moonshot.cn', 'moonshot.ai'], models: [] },
  { id: 'zhipu', label: '智谱', endpoint: 'https://open.bigmodel.cn/api/paas/v4', hosts: ['bigmodel.cn'], models: [] },
  { id: 'volcengine', label: '火山方舟（豆包）', endpoint: 'https://ark.cn-beijing.volces.com/api/v3', hosts: ['volces.com'], models: [] },
  { id: 'siliconflow', label: '硅基流动', endpoint: 'https://api.siliconflow.cn/v1', hosts: ['siliconflow.cn', 'siliconflow.com'], models: [] },
  { id: 'openai', label: 'OpenAI', endpoint: 'https://api.openai.com/v1', hosts: ['openai.com'], models: ['gpt-4o-mini', 'gpt-4o'] },
  { id: 'custom', label: '自定义（其他兼容服务）', endpoint: '', hosts: [], models: [] },
]

const CUSTOM = PROVIDER_PRESETS[PROVIDER_PRESETS.length - 1]

/** The preset a saved endpoint belongs to, by its host; custom otherwise. */
export function presetForEndpoint(endpoint: string): ProviderPreset {
  let host = ''
  try { host = new URL(endpoint.trim()).hostname } catch { return CUSTOM }
  return PROVIDER_PRESETS.find(preset => preset.hosts.some(known => host === known || host.endsWith(`.${known}`))) ?? CUSTOM
}

/** The models offered in the model fields: the preset's usual ones first,
 *  then the ones read from the service, each once. */
export function modelChoices(preset: ProviderPreset, listed: string[]): string[] {
  return [...new Set([...preset.models, ...listed])]
}

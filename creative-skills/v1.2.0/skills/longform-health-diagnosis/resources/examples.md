# 长篇健康诊断：示例

## 合成边界案例 E-I01
- 类型：`inference`（用于评测，不归因于原始材料）。
- 情境：用户要求执行“每20-30章/Arc/Volume/手动”，但缺少 `recent metrics`。
- 期望：返回 `NEEDS_INPUT`，指出最小缺口，不生成权威事实。

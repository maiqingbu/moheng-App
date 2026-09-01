# 完结与回报审计：示例

## 合成边界案例 E-I01
- 类型：`inference`（用于评测，不归因于原始材料）。
- 情境：用户要求执行“最后一章后、Completion 前”，但缺少 `BookExit candidate`。
- 期望：返回 `NEEDS_INPUT`，指出最小缺口，不生成权威事实。

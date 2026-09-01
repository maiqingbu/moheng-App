# Method Precedence & Epistemic Policy v1.1

## 目的

本包同时包含用户提供资料的可追溯证据与本次审查补入的 `project_method`。两者必须严格区分，避免把补充方法伪装成用户原始资料。

## 优先级

1. **HARD / Canon / Knowledge / 用户显式决定**：最高优先，任何 Skill 方法不得覆盖。
2. **Narrative Runtime R1 / ARCHITECTURE_BOUNDARY_R1 / 当前任务合同 / Skill Runtime Contract**：决定可见信息、预算、权限和输出边界。
3. **已批准的 SkillProfile / Genre Overlay / NarrativeTasteProfile**：项目级方法偏好。
4. **source_evidence**：来自本次 67,818 字规范化资料的可追溯方法证据。
5. **project_method**：为弥补资料缺口而补入的通用创作方法；不得声称来自用户原始资料。
6. **model_inference**：模型临时推断，必须可被上层 Gate 拒绝。

## 数字与平台经验

资料中的留存率、完本率、平台规则、固定章数/节奏等数字默认只作为**经验性启发**。除非当前 MarketEvidence 或正式平台规则再次确认，不得升级为 HARD 事实或绝对阈值。

## 示例使用

来源案例只用于抽取结构机制，不复制具体表达、人物或桥段；不得要求模仿在世作者的独特风格。

## 冲突处理

- source_evidence 与 project_method 冲突：保留两者并报告 tradeoff，不静默合并。
- project_method 与项目权威冲突：项目权威胜出。
- 信息不足：`NEEDS_INPUT`；不能用 Skill 知识补成项目事实。

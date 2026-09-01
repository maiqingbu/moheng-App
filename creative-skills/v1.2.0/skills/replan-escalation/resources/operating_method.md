> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 重规划与升级决策：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 先按根因层级分类：句/段表现、场景结构、章节计划、Arc/卷结构、Book/硬设定、外部输入；不得用更大范围掩盖诊断不清。
2. 结合 repair attempts 判断“同一根因是否已尝试不同策略”；重复同一种改法不算新尝试。
3. scope 选择满足修复所需的最小层级：REWRITE_SPAN/SCENE、REPLAN_CHAPTER、REPLAN_ARC、GLOBAL_CHANGE、BLOCK、HUMAN_DECISION。
4. authority impact 或 dependency impact 跨越已提交 Canon/大量后续章节时，必须走 Change/Preflight，不能在 Repair 内偷改。
5. cost_estimate 同时考虑模型成本、重写文本量、依赖重算和已发布内容风险。
6. 达到 repairRunLimit 或预算硬上限后禁止继续自动 ModelRun；必须返回明确决策。
7. human_question 只询问机器无法从权威和策略推导的真正选择，避免把所有困难甩给用户。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 达到上限后 ModelRun 不再增长
- GLOBAL 影响必须走 Change/Preflight
- A/B authority 不得自动越权
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

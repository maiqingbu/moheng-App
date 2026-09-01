> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 完结与回报审计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 先建立 closure ledger：ReaderPromise、A/B obligations、主线问题、人物弧、关键关系、伏笔/谜题分别标 resolved / intentionally_open / invalidated / stale。
2. unresolved_items 必须区分“应该解决却遗漏”和“明确选择留白”；后者需要理由与用户/合同允许。
3. payoff_coverage 不只数数量，还检查重要承诺是否得到与投入匹配的情绪/因果回报。
4. completion_findings 逐项给 evidence refs，禁止凭最后一章印象判断整书完成。
5. Retcon 后重新计算 stale_refs；曾经 PASS 的 CompletionReport 可以失效。
6. completion_recommendation 只有 HARD 完成条件和核心承诺闭合同时满足才 PASS；否则返回最小补完范围。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- A/B 未解决必须 FAIL 或用户批准
- 关键 Retcon 后 Completion 可 stale
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

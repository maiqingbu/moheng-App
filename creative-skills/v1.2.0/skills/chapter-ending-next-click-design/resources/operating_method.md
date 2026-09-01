> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 章节结尾与继续阅读动机设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 章节结尾先完成本章的主要状态变化，再决定是否开启下一期待；不要把“没写完”当 cliffhanger。
2. ending_mode 在决定、发现、代价、反转、兑现后新问题、关系变化、目标升级之间轮换。
3. next_click_reason 必须能说清读者下一章想确认什么，而不是泛泛“悬念更大”。
4. 如果本章已经高压，结尾可用余震/新代价；如果本章平稳，结尾需要更强方向变化。
5. 重复检测：连续多章使用同一断句、突然来人、门外异响等表面钩子时强制变体。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得强制 cliffhanger
- 结尾必须来自本章因果
- 重复模式要触发 repetition warning
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

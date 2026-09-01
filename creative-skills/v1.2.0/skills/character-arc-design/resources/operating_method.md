> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 人物弧设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. arc_question 用一个可被事件检验的问题描述人物变化，例如“他会继续用控制换安全，还是愿意承担信任的风险”。
2. pressure_sequence 逐步攻击人物当前有效但有代价的生存策略，而不是用旁白告诉他该成长。
3. choice_points 必须存在真实替代方案；没有选择就没有人物弧证据。
4. costs 记录每次坚持旧模式或尝试新模式付出的代价。
5. belief_shifts 只能在足够证据后更新；突然顿悟但行为不变不算弧线。
6. evidence_targets 定义后续可观察行为；允许 relapse，但复发必须带来新认识或更高代价。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得把“心理变化”无证据写成 Canon
- 必须指定可观察行为证据
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

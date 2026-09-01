> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# POV 与心理距离设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. pov_holder 决定当前场景可直接感知与思考的主体。
2. distance 定义叙述贴近程度；距离变化应有目的，不在同段无标记跳动。
3. perception_scope 只写 POV 能观察到的外部信息；internal_access 只进入允许角色的内心。
4. forbidden_knowledge 包括未来事实、他人未表达内心、未获得秘密。
5. shift_policy 明确何时可换 POV，通常以场景/章节边界为主；切换必须带来信息或张力价值。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得越过 Knowledge boundary
- 切换 POV 必须符合上层 policy
- 未来计划不可通过叙述泄露
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

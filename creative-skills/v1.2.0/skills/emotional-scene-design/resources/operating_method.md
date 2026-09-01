> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 情绪场景设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. emotional_objective 定义读者/角色需要经历的情绪变化，而不是指定“这里必须哭”。
2. pressure 来自具体损失、选择或关系，不靠形容词堆叠。
3. behavioral_signals 用动作、注意力、语言变化和身体反应证明情绪。
4. suppression_or_expression 根据人物性格与场景安全度决定克制/爆发。
5. turn 改变情绪解释或关系位置；aftereffect 让情绪事件影响后续行为。
6. 避免用悲惨背景替代当前戏剧过程。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得强制“展示不要讲述”绝对化
- 不得无因情绪跳变
- 关键情绪变化要有触发与后效
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

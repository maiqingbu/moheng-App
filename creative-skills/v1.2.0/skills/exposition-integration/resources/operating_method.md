> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 说明信息融入设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 每个 exposition_unit 先问：角色现在做决定是否必须知道；不是就 defer。
2. delivery_modes 优先行动后果、冲突、物件、制度摩擦、对话目的性泄露，再考虑旁白说明。
3. max_load 控制连续新概念数量；专有名词必须有即时语境。
4. action_link 让说明改变角色下一步，而不是暂停故事。
5. 允许先展示效果后解释机制；延迟解释要确保读者仍能理解当前 stakes。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得百科式倾倒
- 角色不知道的信息不能借对话说明
- 可延迟的信息优先延迟
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 人物关系动力设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 关系不是标签，而是双方在资源、情感、秘密、地位和目标上的动态交换。
2. 分别记录 A→B 与 B→A 的需求、信任、恐惧和筹码，避免把关系写成单向好感值。
3. power_balance 说明当前谁能让谁付出代价；权力可以来自信息、资源、身份、情感依赖或道德债务。
4. hidden_tension 至少包含一个双方不能直接说出的矛盾或不对称认知。
5. change_triggers 必须是事件/选择/代价，不允许无因升温或降温；重大关系变化需要可回溯证据。
6. payoff_paths 给出多种可能方向而非锁死情侣/敌友结局，保留长篇适应空间。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得把关系状态当作人物真实心理事实直接写 Canon
- 必须区分外显关系与角色 Knowledge
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

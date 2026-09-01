> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 长篇滚动规划：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 只规划固定近窗，远期保留意图与约束；每次窗口滑动都以最新 Canon/Ledger 为现实。
2. chapter_intents 写目标与状态变化，不提前锁死所有场景。
3. state_targets 覆盖主线、人物、关系、ReaderDesire、伏笔/obligation 的近期动作。
4. risk_watch 监控连续重复、债务积压、升级过快、角色掉线。
5. replan_triggers 定义何种现实偏差需要重算窗口，而不是每章都重做全书总纲。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得预生成 4000 章
- 第100/1000/3000章上下文不随历史线性增长
- Canon 优先于 Summary
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

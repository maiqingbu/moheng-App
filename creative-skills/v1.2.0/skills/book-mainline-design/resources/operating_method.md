> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 全书主线设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 主线按阶段目标与不可逆转折组织，不按“发生了哪些大事件”组织。
2. 每个 phase_goal 必须由前一阶段结果产生，并为下一阶段制造新的约束。
3. opposition_evolution 让对手/制度/环境学习和变化，避免主角永远重复同一种解法。
4. major_payoffs 与前置投入绑定；每个大回报之后产生新的责任、敌人、信息或代价。
5. 支线只有在改变主线资源、关系、认知或选择时才进入主结构。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 只到阶段/卷意图粒度
- 不得预生成逐章事件清单
- 每阶段必须有状态变化而非换地图
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

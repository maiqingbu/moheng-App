> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 卷级结构设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 每卷应有独立可感知的目标、压力曲线和 exit_state_target，同时服务全书主线。
2. internal_phases 至少包含建立、升级、转折/失衡、收束，不要求机械四段等长。
3. major_turns 必须改变卷内策略，不只是增加敌人强度。
4. obligations 记录本卷承诺必须处理的事项；payoffs 说明哪些在卷内兑现、哪些升级到后续。
5. 卷尾要完成一部分并打开下一层，不以突然截断代替交接。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 默认 60-100 章，>120 需重切/批准
- 只设计当前卷
- 不得重复 Book authority
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

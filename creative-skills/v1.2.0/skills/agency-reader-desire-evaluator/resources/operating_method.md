> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 人物主动性与读者期待评估：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. agency 检查谁发起关键行动、谁做不可逆选择、结果是否主要由主角行为产生。
2. passive_response_flags 识别连续被通知、被带走、被救、被安排、只观察不决策。
3. desire_events 记录读者期待的 open/progress/payoff/abandon，不把所有悬念都算 desire。
4. payoff_balance 检查开坑速度与兑现速度；长期只有 progress 无 payoff 会形成疲劳。
5. repair_hints 优先给主角可做的选择、可承担的代价或可主动追求的信息。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得强制每章新增 desire
- 人物被动样例必须能检出
- ReaderDesire 不写 Canon
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

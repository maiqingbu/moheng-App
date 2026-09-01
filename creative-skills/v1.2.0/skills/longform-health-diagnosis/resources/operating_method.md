> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 长篇健康诊断：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. health_report 看趋势而非单章波动；至少比较多个窗口。
2. drifts 分为主线、人物agency、ReaderDesire债务、伏笔债务、重复、节奏、成本、上下文膨胀。
3. trend_evidence 必须可追溯到 ledger/quality/modelrun 数据，不把感觉写成事实。
4. change_proposals 只提出候选，不能直接改 Canon。
5. urgency 依据不可逆风险和传播范围，而不是分数高低。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 运行前后 Canon hash 不变
- 不得直接写权威
- 相关性不得表述成因果
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

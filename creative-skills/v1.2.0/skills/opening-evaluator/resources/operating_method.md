> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 黄金三章评估：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 评估黄金三章整体，不把单章高分等同 Signing。
2. quality_vector 至少覆盖 Hook清晰度、核心卖点体验、主角agency、冲突推进、信息负荷、ReaderPromise、三章差异、第一次兑现、长期扩展性。
3. findings 必须指向章节/场景/文本证据并标 severity。
4. promise_coverage 检查承诺是建立、推进还是兑现；desire_state 检查旧期待是否积压。
5. repetition 同时查功能重复和表面钩子重复。
6. signing_recommendation 只有 HARD 全过且关键质量缺陷可接受时 PASS；否则给最小 repair_scope。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- HARD 与 QUALITY 分离
- 不得因高分覆盖事实错误
- findings 必须定位 evidence/span
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

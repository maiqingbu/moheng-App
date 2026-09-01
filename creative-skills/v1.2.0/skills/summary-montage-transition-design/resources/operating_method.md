> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 概述/蒙太奇/过渡设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. mode 在 scene、summary、montage、transition 中选择，依据是否需要实时选择与情绪体验。
2. compression_goal 说明要压缩什么重复过程；must_show 保留关键转折和代价。
3. may_summarize 适合重复训练、旅行、常规流程和低冲突时间跨度。
4. transition_anchor 用地点、时间、目标、物件或未完成动作建立连续性。
5. 即使概述也必须有 exit_state 变化，避免纯时间跳过。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- SUMMARY/MONTAGE 不因缺少强 sceneTurn 被 HARD BLOCK
- 不得把关键选择压缩掉
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

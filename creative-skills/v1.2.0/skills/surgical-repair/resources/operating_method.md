> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 局部修复：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 先锁定 finding 的最小因果范围，再确定 target_spans；不要默认整章重写。
2. protected_spans 包含已正确的 Canon 事实、伏笔、人物声线、关键回报和用户明确保留内容。
3. changes 每项说明目的、修改点和预期指标，不接受“整体润色”。
4. 修复后只重检受影响维度 + HARD Gate，避免每次全链无限重跑。
5. 若同一根因连续修复达到上限仍失败，停止 ModelRun 并交给 S41。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得默认整章重写
- 非关联 protected span hash 应保持
- 不得私自修改上层事实
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 章节叙事质量评估：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. quality_vector 分维度评分，但评分必须附 evidence_spans。
2. 优先找会改变读者体验的根因：目标不清、主角被动、冲突无反制、信息过载、转折无因、情绪无铺垫、回报无建立。
3. severity 与 scope 分开：严重问题未必需要大改，大范围问题也可能只是节奏统一调整。
4. repair_priority 按 HARD→因果/agency→ReaderPromise→场景结构→语言表面排序。
5. 不要因为文句漂亮掩盖场景没有状态变化。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- NORMAL 默认合并式 evaluator
- HARD 与 QUALITY 分离
- findings 必须可定位
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 长期升级与压力递增设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 升级至少从威胁、信息、资源、关系、规则、时间压力、道德代价中选择多个轴，不只抬数值。
2. thresholds 定义何时旧策略失效，迫使角色学习或重组资源。
3. counterpressure 让世界对主角成长产生响应，避免升级成为单向通胀。
4. cost_growth 与能力增长并行；能力越强不意味着选择越轻松。
5. novelty_sources 优先来自新组合和新约束，不靠不断换地图/换敌人皮肤。
6. ceiling_risks 提前标记无法继续升级的轴，避免后期失控。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 至少两个非数值升级轴
- 不得靠无限抬战力解决
- 必须保留反制和失败可能
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# Hook 与好奇缺口设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. Hook 的目标是形成清晰问题或未完成动作，不是让读者什么都看不懂。
2. information_gap 必须同时满足：读者知道自己缺什么、缺口值得知道、未来存在合理兑现窗口。
3. Hook 尽量绑定人物欲望或 stakes；纯世界谜语若与主角无关，吸引力不稳定。
4. reader_questions 分层为即时问题、章节问题、长期问题；同层同时开放过多会造成认知拥堵。
5. 每个问题定义 payoff_window，超过窗口仍无进展则转为拖延风险。
6. 用可理解性测试：删掉专有名词后读者是否仍知道“谁遇到什么问题、为什么要继续看”。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得靠无意义断章制造假悬念
- 必须说明读者为什么在意
- Hook 不得违反 Knowledge/visibility
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

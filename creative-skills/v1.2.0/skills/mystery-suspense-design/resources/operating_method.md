> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 谜团与悬念设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. mystery_questions 分层：核心谜题、阶段谜题、场景问题；每层都要知道何时至少推进一次。
2. clues 分为直接线索、背景证据、行为异常、缺失信息；关键答案必须可由既有线索回溯解释。
3. red_herrings 必须来自合理替代解释，不能靠作者隐瞒 POV 已知事实。
4. reveal_windows 规定最晚推进窗口，避免永久拖谜。
5. knowledge_boundaries 严格区分读者、侦查者、嫌疑人/对手掌握的信息。
6. fairness_checks：答案揭晓后，读者应能指出至少两处此前可见但未必显眼的支持证据；反转不能凭新规则凭空出现。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得用作者未定义答案维持伪谜团
- 关键答案要有 authority boundary
- 不得提前泄露
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

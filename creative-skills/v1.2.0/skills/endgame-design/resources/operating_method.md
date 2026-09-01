> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 终局设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. endgame_contract 从 ReaderPromise、主线、人物弧、关键伏笔和 obligations 汇总必须解决项。
2. must_resolve 与可有意留白分开；留白必须不破坏核心承诺。
3. final_choices 让主要人物通过选择完成弧线，不用旁白总结代替。
4. payoff_sequence 安排多个回报的因果和情绪顺序，避免所有答案在一章清单式宣布。
5. forbidden_shortcuts 包括新规则救场、反派突然降智、关键伏笔临时补写、无代价复活等。
6. completion_preconditions 未满足时不得因为“最后一章已生成”而完结。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 未解决 A/B obligation 不得假完成
- 不得临时新增万能设定解决问题
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

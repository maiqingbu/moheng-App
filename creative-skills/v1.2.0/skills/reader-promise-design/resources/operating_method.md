> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 读者承诺与期待设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 把 Reader Promise 写成“读者持续投入后会稳定获得什么体验”，而不是剧情剧透。
2. 区分 desire 类型：结果欲望（想看赢/逃/复仇）、认知欲望（想知道真相）、关系欲望（想看关系变化）、能力欲望（想看成长/掌控）、情绪欲望（想获得爽/甜/虐/治愈）。
3. 每个主要承诺都要定义最早信号、阶段性小兑现、长期大兑现和不可违背边界。
4. 悬念只能延迟答案，不能替代承诺；连续开新问题但不兑现会形成债务。
5. 兑现后立即判断是否需要开启更高层期待，避免“爽点结束=故事失速”。
6. anti_promises 明确列出项目不承诺的体验，防止后续为了追热点破坏定位。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- Reader Promise 不是 Canon
- 不得强制每章 cliffhanger
- 输出必须可被后续 ReaderDesire evaluator 使用
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

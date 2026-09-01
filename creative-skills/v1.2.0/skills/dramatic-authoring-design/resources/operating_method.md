> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 戏剧化创作设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 从 want 开始，不从事件清单开始；场景必须有人试图改变局面。
2. opposition 产生反行动，形成 tactics↔countermoves 链；每次行动应改变可选策略。
3. stakes 说明为什么现在必须行动；cost 说明失败或成功都可能付出的东西。
4. forced_choice 在关键节点制造不可兼得，避免主角总有无损最优解。
5. turn 是信息、权力、目标或关系的方向变化，不只是更大爆炸。
6. agency_chain 检查主要转折是否由人物选择触发；若全由巧合推动，标记修复。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得新增 SceneContract 外事实
- 没有 agency_chain 不得进入 Renderer（除明确 SUMMARY/MONTAGE）
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

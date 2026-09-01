> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 章节计划设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. chapter_goal 用本章结束时必须发生的状态变化表达，不用“推进剧情”。
2. events 只列必要因果节点；choices 标出谁做决定以及可选项。
3. state_changes、knowledge_changes、obligation_moves 分开，防止章纲只有动作没有叙事后果。
4. exit_conditions 说明本章何时算完成，防止 Renderer 无限制延长。
5. scene_modes 用于节奏与表现选择，不把章纲写成逐句剧本。
6. 若本章删掉后前后状态几乎不变，重新定义功能或合并。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得写成逐段正文脚本
- 不得新增上层未授权事实
- 必须有可验证 exit conditions
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

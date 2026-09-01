> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 配角群像设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 每个重要配角至少有独立 want、可失去之物和不依附主角也会采取的行动。
2. dramatic_function 只描述其对故事的主要作用，但人物不能等同于功能；同一人物可在不同阶段改变功能。
3. 用 contrast_axes 与主角/其他配角形成价值观、方法、资源、身份或情绪温度差异，避免同声线群像。
4. 建立 conflict_links：谁阻碍谁、谁需要谁、谁欠谁、谁误解谁；至少一条关系要能在没有反派时制造戏。
5. entry_priority 由当前阶段的必要性决定；不为“人物齐全”一次性引入过多名字。
6. 做冗余测试：两个配角若欲望、功能、声线、关系位置高度重叠，合并或重构。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得一次性制造全书角色百科
- 只设计当前阶段必要角色
- 每个关键配角必须有独立欲望而非只服务主角
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

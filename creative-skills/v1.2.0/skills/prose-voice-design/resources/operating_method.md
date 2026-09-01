> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 文风与叙述声音设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. voice_targets 用属性向量描述：叙述距离、正式度、速度、幽默温度、感官偏好、抽象/具体比例；不要用作者名代替。
2. sentence_rhythm 根据场景功能定义句长与停顿变化，不追求全书单一节拍。
3. lexical_density 控制专有名词、修饰词和抽象词密度；关键动作节点优先清晰。
4. imagery_policy 指定比喻/意象主要来源与禁用陈词滥调，但不强迫每段使用。
5. humor_register 说明幽默来自人物、观察、反差还是语言；不适合的场景允许为零。
6. taboos 包含模仿在世作者独特风格、空泛华丽、同义堆叠、AI式总结句、角色同声线等。
7. variation 允许不同 POV/场景在同一总声线下有局部变化；风格一致不等于句式重复。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得模仿在世作者的独特风格
- 目标用属性描述而非作者名
- 不得覆盖 HARD 事实
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

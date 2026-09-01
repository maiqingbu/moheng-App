> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 主角设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 主角先定义可观察的身份核心与当前生存方式，再定义目标；不要从性格标签开始。
2. want 必须能驱动近期主动行动；need_or_gap 是更深层的认知/情感/能力缺口，两者可以冲突。
3. strengths 与 flaws 尽量成对设计：同一特质在不同压力下既能解决问题也能制造代价。
4. 至少设计一组内在矛盾，例如渴望控制却需要信任、追求自由却依赖认可；矛盾必须能落到选择。
5. agency_engine 说明主角为什么会主动制造下一件事：目标、资源、方法偏好、不可接受底线、失败后替代策略。
6. growth_space 不预写固定结局，而定义可被事件检验的变化轴和证据；成长必须由选择—后果—再选择累积。
7. 用替换测试：把主角换成一个性格完全不同的人，主要事件若仍原样发生，则主角设计未进入因果链。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得以人物标签替代行为机制
- 不得与已提交开篇 Canon 冲突
- 必须给出可产生主动选择的 agency_engine
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 人物登场设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 人物第一印象优先由“在压力下做了什么选择”建立，而不是履历介绍。
2. entrance_goal 只保留当前场景必须建立的1-3个印象，不试图一次讲完背景。
3. action_signal 同时暴露能力与代价：角色擅长什么、习惯怎么解决问题、这种方法有什么风险。
4. 用一个可辨识的 contrast 建立记忆点，但避免把人物压缩成口癖或外貌标签。
5. relationship_signal 通过对另一个人的具体态度/行为建立关系位置，而不是旁白说明。
6. avoid_exposition 列出本次登场明确不讲的信息；若信息可延迟，就让后续事件证明。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得用人物小传直接转正文
- 优先行为证据
- 登场不得泄露角色未知秘密
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

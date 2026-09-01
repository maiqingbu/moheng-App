> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 场景契约设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. scene_goal 写成“场景结束时哪项状态必须改变”，而不是“人物聊天/打一架”。
2. participants 只包含对目标有作用的人；每个主要参与者应有当场 want。
3. allowed_facts 与 forbidden_facts 明确 Renderer 可用和不可用的信息，尤其 POV/未来秘密。
4. knowledge_boundary 分离角色已知、读者已知和作者/系统已知。
5. required_outcomes 只冻结必要结果，不冻结实现路径，给 Authoring 保留策略空间。
6. scene_mode 决定表现倾向，但不能覆盖 HARD 事实。
7. 场景自检：进入状态→欲望→阻力→策略/反策略→转折→代价→退出状态，至少有一项不可逆变化。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得新增上层事实
- Renderer 只能看到可见 capsule
- HARD 来源不足必须 BLOCK
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

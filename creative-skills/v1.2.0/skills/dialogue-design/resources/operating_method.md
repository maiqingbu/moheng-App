> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 对话设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 每段重要对话先写 dialogue_objectives：每个人想让对方做、信、承认或隐瞒什么。
2. subtext 是“真正目标与表面话语的差”，不是故意说谜语。
3. voice_markers 用词汇偏好、句长、直接度、礼貌策略、比喻来源和回避方式区分角色，避免只靠口头禅。
4. power_shifts 记录每轮谁获得信息、承诺、让步或地位优势；长对话若权力不变容易变成说明会。
5. withheld_info 必须符合角色动机和知识边界；角色没有理由隐瞒时不要硬拖。
6. 信息尽量通过冲突、误解、谈判和选择传递；连续问答式百科说明列入 avoid_lines。
7. 对白删去姓名标签后仍应有较高可辨识度，但不要追求每句都夸张个性。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得让角色说出其不知道的信息
- 不得所有角色同一语气
- 信息说明必须服务当下行动
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

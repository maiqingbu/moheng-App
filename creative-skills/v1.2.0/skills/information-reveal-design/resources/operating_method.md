> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 信息释放与秘密控制：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 为每条关键信息标记 reader_knows、pov_knows、其他角色 knows，避免越权全知。
2. reveal_plan 按“信号→部分解释→验证/反转→完整答案”组织，不要求所有信息都走同样节奏。
3. withheld_info 必须有叙事理由；仅为了拖延而让 POV 不想/不看显而易见事实属于作弊。
4. 戏剧反讽只在读者多知道能增加张力时使用；若只消灭悬念则不划算。
5. forbidden_reveals 作为 Renderer 硬边界，不因文采需要泄露未来 Canon。
6. 解释发生在角色需要做决定的节点最有效；能由行动后果展示的信息优先不讲解。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 必须区分 reader knowledge / POV knowledge
- 不得把未来计划塞进 Renderer capsule
- 解释不能抢占当前戏剧行动
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

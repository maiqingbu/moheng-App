> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 黄金三章后的故事架构综合：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 先从已完成黄金三章提取“实际成立的故事发动机”，区分原计划与正文证据；总纲不能否定已经签署的开篇现实。
2. 至少生成2-4套结构候选，差异必须体现在主线推进机制、阶段划分、人物轨迹或升级模型，而非换卷名。
3. mainline 用因果链描述：目标→阻力→选择→不可逆变化→新目标，不用事件清单堆积。
4. major_phases 每阶段都要改变资源、关系、认知或身份，使下一阶段问题不能用上一阶段同一方法解决。
5. character_trajectories 与主线互相施压，避免人物弧成为附属总结。
6. ending_boundary 只定义必须抵达的状态和不可丢失的承诺，不提前锁死每一步。
7. tradeoffs 明确每个候选的优势、风险、所需伏笔和对黄金三章兼容性，交给用户/上层 Gate 选择。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 默认生成 2-4 个架构候选
- 不得否定已提交 Canon
- 冲突必须明确标记为 Retcon/Change 候选
- 不得预生成 4000 章细纲
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

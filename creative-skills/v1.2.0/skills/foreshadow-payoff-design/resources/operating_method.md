> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 伏笔与回收设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. plant 必须既能在首次出现时自然成立，又能在回看时获得第二层意义。
2. reinforcement 不是重复提醒，而是在新语境中增加可见度或改变解释。
3. recovery_window 根据重要度定义最早/最晚回收区间；长期伏笔需防遗忘。
4. payoff_types 区分事实揭示、能力使用、关系回响、主题回响、反转。
5. 每个重要回收必须能追溯 evidence_refs；Retcon 后检查 staleness。
6. 避免把所有普通细节登记为伏笔，降低账本噪声。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得要求每章埋伏笔
- 不得与 ReaderDesire/Obligation 合并
- 必须可追踪 evidence
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 动作/冲突场景设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 先建立 space_model、目标和可用资源，读者必须知道谁在哪里、要什么。
2. tactics 与 counterplay 交替，每轮改变位置、资源、信息或目标。
3. 动作能力必须遵守既有规则与 costs；禁止临时新技能救场。
4. turns 优先来自策略、环境或信息变化，不只来自更强敌人加入。
5. outcome 必须改变故事状态；若战斗结束后一切恢复原样，压缩或删除。
6. 空间清晰度优先于招式数量。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得违反能力体系 HARD rules
- 不得只堆招式
- 必须产生状态后果
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 重复与疲劳评估：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 区分表面重复（词句/钩子）与结构重复（同一目标—阻力—解决模式）。
2. repetition_clusters 按连续窗口和长周期两层检测。
3. novelty_gaps 从冲突来源、关系组合、场景模式、信息结构、代价、解法中寻找缺口。
4. safe_variations 不能破坏角色/世界规则，只改变表现和问题组合。
5. severity 取决于读者是否能提前预测过程，而不只是出现相似元素。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得仅用 n-gram 相似度代替叙事判断
- 必须指出重复模式的功能层级
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

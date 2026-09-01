> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 开篇策略搜索：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 先生成至少三种机制不同的开篇策略，而不是同一情节换措辞；可用危机、异常、欲望、关系破裂、谜团、身份反差等不同入口。
2. 每个策略只做低成本骨架：首个可见问题、主角首次主动选择、核心卖点首次体验、前三章小兑现、留下的长期问题。
3. compatibility_scores 分开评估 ReaderPromise、主角 agency、世界规则负荷、信息复杂度、题材惯例和长期扩展性。
4. 优先选择能在第一章展示“这本书独特体验”的策略，而不是最吵闹的策略。
5. long_horizon_viability 至少给出三条后续发展方向，证明开篇不是一次性 gimmick；但不得提前生成完整总纲。
6. 若多个策略接近，保留前二进入短推演，而不是过早单点锁死。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 至少 3 个策略候选
- 只允许低成本策略草图默认并行
- 必须评估长期可扩展性但不生成总纲
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

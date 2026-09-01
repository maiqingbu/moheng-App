> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 核心冲突与代价设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. conflict_engine 由“主角想要什么 + 谁/什么持续阻止 + 为什么不能绕开 + 失败代价”组成。
2. opposition_sources 至少区分人物、制度、环境、内在缺陷和资源限制，避免全靠新反派。
3. stakes 分层：即时、关系/身份、阶段目标、长期价值；不要每次都升级成生死世界毁灭。
4. forced_choices 要让两个有价值目标不能同时满足；真正的代价来自取舍。
5. 升级优先改变信息、资源、关系、规则和选择空间，再考虑纯数值。
6. 主角必须通过选择推动冲突下一阶段；若事件只追着主角跑，标记 agency 风险。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 必须保留人物主动选择空间
- 不得把 stakes 只写成抽象“世界毁灭”
- 至少提供局部/关系/长期三层 stakes
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

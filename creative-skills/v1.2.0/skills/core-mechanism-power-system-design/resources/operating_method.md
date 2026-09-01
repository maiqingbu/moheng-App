> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 核心机制/能力体系设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 先定义机制给读者的核心体验，再定义能力清单；机制必须服务选择、冲突和回报。
2. rules 写清输入、输出、触发条件与可观察后果；limits 写不能做什么，costs 写做了付什么。
3. progression_axes 至少包含两种非同质成长维度，例如熟练度/资源/权限/理解/组合，而不是只有数值变大。
4. counterplay 要让对手能学习、限制、诱导或利用机制，否则冲突会退化为单方面碾压。
5. 每次升级都检查是否改变策略空间；只提高伤害数字而不改变选择，视为低质量升级。
6. reveal_schedule 只承诺逐步暴露规则，不提前把所有上限和秘密写死。
7. 做漏洞测试：无限循环、无代价复活、信息全知、资源自增、规则例外是否能摧毁 stakes。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得提前泄露 forbidden reveal
- 必须有 counterplay 与 cost
- 不得把升级表直接等同故事总纲
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

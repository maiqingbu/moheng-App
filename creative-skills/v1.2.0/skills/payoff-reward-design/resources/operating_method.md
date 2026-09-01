> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 回报与爽点设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 回报必须回答一个此前真实建立的欲望/投入，不能凭空发奖。
2. setup_requirements 记录读者为什么会觉得这个回报“值得”。
3. payoff_types 在胜利、身份、资源、关系、认知、情绪、能力掌控间变化。
4. magnitude 与铺垫、代价和延迟匹配；小铺垫不应硬抬成宇宙级高潮。
5. timing 避免承诺过久无进展，也避免刚建立就完全兑现。
6. aftereffects 让回报改变后续问题，否则爽点成为可删除插曲。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得无铺垫发奖
- 不得长期只开期待不兑现
- 回报后要更新状态而非原地循环
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

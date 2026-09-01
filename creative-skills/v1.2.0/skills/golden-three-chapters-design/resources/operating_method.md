> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 黄金三章整体设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 三章必须分工而非重复：第1章建立核心体验与问题，第2章扩大选择/机制/关系，第3章形成第一次有重量的兑现或转折；具体分工可按题材调整。
2. promise_progression 要让 ReaderPromise 从“信号”升级到“证据”，至少有一次小兑现。
3. conflict_progression 每章都改变局面；若三章只是同一冲突连续拖长，视为结构重复。
4. reveal_progression 控制专有名词和背景负荷，优先让读者先理解欲望与风险。
5. desire_progression 记录旧期待是否推进、兑现或升级；禁止只开新坑不处理旧期待。
6. signing_targets 应是可观察条件：主角 agency 已建立、核心卖点已体验、至少一条长期问题成立、无硬设定冲突、后续存在多个扩展方向。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 不得预设完整全书总纲
- 三章必须形成可评估的实际作品证据
- 第 3 章结束要留下多个可行长期方向而非锁死唯一总纲
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

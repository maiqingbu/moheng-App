> v1.2 Architecture Boundary：本方法只处理 Creative/SOFT 层。项目事实由 Runtime 提供的 bounded SkillTaskContext 决定；不得直接查询 DB/Authority。continuity/knowledge/reservation/task/integrity 的 HARD 核验必须交给 Runtime/Support Harness。

# 世界观与硬规则设计：运行方法 v1.1

> 来源类型：`project_method`。这是本次验收为补足/校正 Skill 可执行性加入的方法，不声称来自用户缺失的原始资料。

## 决策顺序
1. 硬规则优先定义“不能做什么、做了要付什么代价、谁能验证”，因为限制比百科条目更能产生剧情。
2. 把设定分成 hard_rules、soft_assumptions 和 expansion_slots；未提交的软假设不能伪装成 Canon。
3. scarcity 与 costs 必须能影响人物选择；无限资源会削弱冲突。
4. institutions 由规则和稀缺自然生长：谁控制资源、谁解释规则、谁从制度获利。
5. 每条关键规则做破坏测试：若角色可无代价绕过，规则不够硬；若规则堵死所有策略，缺少可玩空间。
6. 信息释放按当前故事需要渐进，不把世界观完整性等同于一次性说明。

## 使用来源证据时
- `principles.md` 与 `decision_rules.md` 中的 WWC 资料仅作证据/启发；必须先检查语义是否真的对应当前输出字段。
- 禁止“因为某段被检索到，就机械绑定到某个 payload 字段”。
- 数字、平台经验、案例遵循 `../../contracts/METHOD_PRECEDENCE.md`。

## 最终检查
- 开篇前只冻结必要 HARD rules
- 不得用百科式世界设定挤占上下文
- 新增规则不得逆向破坏已提交 Canon
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 结论必须说明关键假设与不确定性。
- 能局部修复时不扩大到更高层级；确需升级则给 `nextSkillHints`。

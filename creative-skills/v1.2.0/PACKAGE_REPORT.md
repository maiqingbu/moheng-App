# 墨衡 Creative Skills v1.2 架构对齐版

- Skills：45（ID/slug 保持稳定）
- Package：`1.2.0`
- Basis：Narrative Runtime R1 FROZEN + B05 FINAL PASS `315f21351e3f388d37717431732ac9e0ece9f831`
- 原始方法来源：20 篇、620 个稳定来源段、67,818 个规范化正文字符（不变）
- LOW coverage `project_method`：13（历史保留）

## v1.2 关键修订

1. 全部 45 Skill 改为 Runtime-supplied bounded `SkillTaskContext`；禁止 Skill 直接查 Project DB/Authority。
2. `SkillResult.status` 改为 `OK|NEEDS_INPUT|NOT_APPLICABLE|ADVISORY_ESCALATION`，移除 Skill 的 BLOCK/REPLAN 控制权。
3. 新增 `contextRefs` 与 `runtimeCheckRequests`，把疑似 HARD 问题回送 deterministic Runtime/Support Harness。
4. Formal `RepairPlan` 名称归 Project Support Harness；Creative Skill 只产 `RepairSuggestion/EscalationAdvice`。
5. S17/S25/S41/S42/S43/S44/S45 完成 Runtime overlap 重构。
6. Registry 与每个 `moheng.skill.json` 的 skill version 统一为 1.2.0，修复 v1.1 registry 仍写 1.0.0 的版本漂移。
7. 所有 schema 中“待 B05 冻结”的陈旧说明改为 **待 B10 Skill Runtime contract 冻结**。

## 当前定位

v1.2 是 **B10 Creative Skill & Narrative Quality Runtime 的 architecture-aligned input baseline**。

它现在可以提前冻结方法包边界，但不表示 B10 Runtime 已实现，也不表示模型质量 A/B 已验证。same-model A/B 必须在 B10 正式执行。

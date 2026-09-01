# Open Questions — v1.2

## 已确认
- B05 已 FINAL PASS，source baseline：`315f21351e3f388d37717431732ac9e0ece9f831`。
- Narrative Runtime R1 已明确：B05 不实现 Skill；Creative Skill Runtime 在 B10。
- 45 个 Skill ID/slug 保持稳定；13 个 LOW coverage 的 `project_method` 历史保留，不伪造 provenance。
- Skill 不再拥有项目真相查询、HARD BLOCK、REPLAN 或 formal RepairPlan 权限。
- S17/S25/S41/S42/S43/S44/S45 已按 B07/B11/B12 重新收边界。

## B10 前仍需冻结
- 45 个 payload 子字段的最终精确 JSON 类型；v1.2 先冻结 envelope 与架构边界。
- `SkillTaskContext` 在 B10 composition layer 的正式 Rust/TS schema。
- NarrativeTasteProfile / SkillProfile 的正式持久化合同。
- same-model A/B benchmark、canary、default promotion 阈值。

## 不再属于 Skill 的问题
- continuity checking
- knowledge checking
- reservation enforcement
- task progress truth
- integrity checking
- formal route replan / completion readiness

这些由 B06–B12 Runtime / Project Support Harness 处理。

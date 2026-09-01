---
name: information-reveal-design
description: "决定何时告诉读者什么、何时只给信号，控制解释、谜团与 forbidden reveal。 适用于“前三章”；不用于直接修改项目权威或替代相邻 Skill。"
---

# 信息释放与秘密控制

Skill ID：`S12`　角色：`planner`　优先级：`P0`

## Use when
- 前三章
- 谜团章节
- 重大揭示前

## Do not use when
- Runtime 提供的 `SkillTaskContext` 缺少下列必要输入时；返回 `NEEDS_INPUT`，本 Skill 不自行查询项目 Authority。
- 用户要求直接写入 Canon、FinalChapter、BookPlan、Knowledge 或其他长期权威时。
- 任务主要属于另一个 Skill；返回 `nextSkillHints`，不要扩大本 Skill 边界。

## Runtime boundary
- 只读取 Narrative Runtime/B10 composition layer 提供的 bounded `SkillTaskContext`；本 Skill 不直接查询 Project DB/Repository/Authority。
- Canon/Knowledge/Thread/Task/Reservation/Integrity/Plan stale 的 HARD truth 由 deterministic Runtime / Support Harness 决定。
- 疑似 HARD 问题只能加入 `runtimeCheckRequests`；本 Skill 不下 HARD BLOCK，不执行 replan/commit。

## Required inputs
- OpeningContract
- KnowledgeState
- WorldRules
- ReaderQuestions

## Workflow
1. 只读取 Runtime 提供的 bounded `SkillTaskContext` / ContextManifest capsules 与预算；先做适用性和输入完整性检查，不直接查询项目 DB/Repository。
2. 读取 `resources/operating_method.md` 作为默认执行方法，并记录实际加载资源。
3. 只有需要核对用户原始资料依据时才读取 `resources/principles.md` / `resources/decision_rules.md`。
4. 用 `resources/anti_patterns.md` 检查失败模式；需要示例时才读取 `resources/examples.md`。
5. 将 `source_evidence`、`project_method` 与 `model_inference` 分开记录；来源不足时不得提高置信度。
6. 按 `schemas/output.schema.json` 返回 `SkillResultEnvelope`；状态只使用 `OK|NEEDS_INPUT|NOT_APPLICABLE|ADVISORY_ESCALATION`。
7. 用 `resources/eval_rubric.md` 做 final checks；只输出 CreativeProposal、Candidate/CandidatePatch、QualityFinding、RepairSuggestion 或 EscalationAdvice。

## Output
- 必须包含字段：`reveal_plan`, `withheld_info`, `reader_knows`, `pov_knows`, `signals`, `forbidden_reveals`。
- 必须携带 `sourceRefs`、`contextRefs`、`assumptions`、`runtimeCheckRequests`、`confidence` 与 `nextSkillHints`。
- 输出必须通过 `schemas/output.schema.json`。

## Final checks
- 疑似 HARD 冲突不得由 Skill 定案；写入 `runtimeCheckRequests`。
- 必须区分 reader knowledge / POV knowledge
- 不得把未来计划塞进 Renderer capsule
- 解释不能抢占当前戏剧行动
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- 不得直接写入项目权威；不得生成 Support Harness formal RepairPlan。

## Resources
- `resources/operating_method.md`：默认执行方法（project_method）。
- `resources/principles.md`：来源证据与适用边界。
- `resources/decision_rules.md`：来源候选规则，仅在语义匹配时使用。
- `resources/anti_patterns.md`：来源反模式与项目级禁区。
- `resources/examples.md`：来源案例与合成测试情境。
- `resources/eval_rubric.md`：输出与边界验收。
- `provenance/source_map.jsonl`：本 Skill 的来源映射。
- `../../contracts/SKILL_RUNTIME_CONTRACT.md`：共享运行时合同。
- `../../contracts/METHOD_PRECEDENCE.md`：方法来源、数字经验与冲突优先级。

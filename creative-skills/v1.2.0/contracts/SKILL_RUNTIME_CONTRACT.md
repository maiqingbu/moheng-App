# 墨衡 Creative Skill Runtime Contract v0.3

## 核心原则

1. Skill 是可组合的创作方法模块，不是权威数据库、Continuity Engine 或诊断数据库。
2. 每个任务默认只加载 **1 个 Primary Skill + 0~3 个 Support Skill**；超过 4 个必须由 B10 composition layer 给出预算理由。
3. Skill discovery 先读 registry metadata；`SKILL.md` 轻量，方法正文 progressive disclosure。
4. Skill **无直接 Project DB/Repository 查询能力**，只读取 Runtime 提供的 bounded `SkillTaskContext`。
5. Skill 只输出 `CreativeProposal/Candidate/CandidatePatch/QualityFinding/RepairSuggestion/EscalationAdvice`，不生成 Project Support Harness 的 formal `RepairPlan`。
6. Skill 不拥有 `BLOCK/REPLAN` 控制权。HARD Gate、Thread/Task/Reservation/Integrity/Plan stale 由 deterministic Runtime / Support Harness 决定。
7. Skill version、bundle digest、实际加载 resources、method provenance 必须记录在 SkillInvocation evidence。
8. HARD Gate 与 Narrative Quality 分离；高质量评分永远不能覆盖 HARD 失败。

## SkillResultEnvelope v0.3

```json
{
  "skillId": "S13",
  "skillVersion": "1.2.0",
  "bundleDigest": "sha256:...",
  "taskId": "...",
  "status": "OK|NEEDS_INPUT|NOT_APPLICABLE|ADVISORY_ESCALATION",
  "sourceRefs": [],
  "contextRefs": [],
  "assumptions": [],
  "payload": {},
  "findings": [],
  "runtimeCheckRequests": [],
  "confidence": 0.0,
  "nextSkillHints": []
}
```

`sourceRefs` = 方法来源 provenance。  
`contextRefs` = Runtime 已提供的项目事实/证据 refs。

`runtimeCheckRequests` 只允许请求上层核验，不代表 HARD 结论。首批 kind：

```text
CANON_CONFLICT
KNOWLEDGE_BOUNDARY
THREAD_STATE
TASK_REALITY
RESERVATION
INTEGRITY
PLAN_STALE
EVIDENCE_GAP
```

## Skill 装载策略

- Harness/Runtime 先看 registry metadata，不加载 45 个 Skill 全文。
- 选择 Primary 后加载其 `SKILL.md`，再按需加载 `resources/operating_method.md`。
- `principles.md/decision_rules.md` 仅在需要核对方法来源时读取；示例与 rubric 再按需加载。
- Evaluator 不默认读取 Authoring Skill 全部资源，避免同源自证。
- 运行结束只持久化 structured SkillInvocation / refs / digests，不把 agent transcript 当长期权威。

## 权限边界

Creative Skill 永久不具备：

```text
PROJECT_DB_QUERY
ARBITRARY_SQL
AUTHORITY_WRITE
COMMIT_BUNDLE_WRITE
THREAD_LIFECYCLE_WRITE
TASK_TRUTH_WRITE
RESERVATION_ENFORCEMENT
INTEGRITY_DECISION
ROUTE_REPLAN_EXECUTION
CHANGE_REQUEST_EXECUTION
```

详见 `ARCHITECTURE_BOUNDARY_R1.md` 与 `SKILL_TASK_CONTEXT.md`。

## 版本治理

- Package/Skill 使用 SemVer；项目可 pin SkillProfile。
- 新版本先离线 fixture validation，再在 **B10 Skill Runtime** 做 same-model A/B、canary、default promotion。
- 质量回退按 Skill version 回滚，不重写历史 SkillInvocation。

## 方法来源治理

- `source_evidence`：来自用户已恢复的 67,818 字规范化资料，可追溯到 `WWC-*`。
- `project_method`：审查补强方法；不得冒充原资料。
- `model_inference`：临时推断，永远不能升级为项目事实。
- 运行时遵循 `METHOD_PRECEDENCE.md`。

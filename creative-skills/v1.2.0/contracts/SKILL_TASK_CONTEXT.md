# SkillTaskContext Contract v0.1

Skill Runtime 在加载 Primary/Support Skill 之前，由 Narrative Runtime / B10 composition layer 生成 bounded context。

```json
{
  "taskId": "...",
  "projectId": "...",
  "projectRevision": 0,
  "runtimeStage": "...",
  "contextManifestRef": "...",
  "contextRefs": [],
  "capsules": {},
  "qualityInputs": {},
  "methodProfile": {},
  "budget": {"inputTokens": 0, "outputTokens": 0, "modelRuns": 0},
  "allowedOutputs": [],
  "forbiddenCapabilities": [
    "PROJECT_DB_QUERY",
    "AUTHORITY_WRITE",
    "THREAD_LIFECYCLE_WRITE",
    "TASK_TRUTH_WRITE",
    "RESERVATION_ENFORCEMENT",
    "INTEGRITY_DECISION"
  ]
}
```

## Rules

1. Skill 只能看 `SkillTaskContext` 与被选择 Skill 的 progressive resources。
2. `contextRefs` 指向项目事实/证据；`sourceRefs` 仍专指方法来源 provenance。
3. Skill 不得通过工具绕过 context compiler 读取全项目。
4. Context 不足返回 `NEEDS_INPUT`；由 Runtime 决定是否追加 bounded retrieval。
5. project revision 改变后，旧 SkillTaskContext 默认 stale。

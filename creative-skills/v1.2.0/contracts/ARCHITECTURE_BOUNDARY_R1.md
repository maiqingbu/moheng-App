# Creative Skill ↔ Narrative Runtime Architecture Boundary R1

**Status: NORMATIVE FOR SKILL PACKAGE v1.2**  
**Basis: Narrative Runtime R1 FROZEN + B05 FINAL PASS (`315f21351e3f388d37717431732ac9e0ece9f831`)**

## 1. Ownership

Creative Skill 只负责“怎么创作得更好”，不负责“项目真实是什么”。

以下真相/状态永久由 deterministic Narrative Runtime / existing Authority owner / Support Harness 负责：

- Canon / CanonRelation / CanonKnowledge / CanonEvent truth；
- Continuity Thread lifecycle / source evidence；
- Future Reservation enforcement；
- Task Reality / TaskProgress truth；
- Plan stale / stateDigest；
- Projection integrity / DB integrity；
- Volume close readiness / Completion readiness；
- 正式 recompile / replan / ChangeRequest / Repair execution。

## 2. Skill 输入边界

Skill 不拥有 DB/Repository/SQL/FrontendPort 查询能力。Skill 只接收 Runtime 预先编译的 bounded `SkillTaskContext`，其中可以包含：

- `ContextManifest` / 当前 ChapterPlan / Soft Rolling Horizon；
- 经过 Runtime 过滤的 Canon/Knowledge/Thread/Task/Reservation capsules；
- project evidence refs；
- NarrativeTasteProfile / ReaderPromise / quality features；
- 任务预算与允许输出类型。

缺失项目事实时：`NEEDS_INPUT`。禁止自行补查、补写或把方法知识当项目事实。

## 3. HARD 与 SOFT

Skill 输出是 SOFT creative method result。

Skill 可以发现“疑似需要硬核验”的情形，但只能写入 `runtimeCheckRequests[]`；真正的 HARD PASS/FAIL/BLOCK 由 Runtime Gate / Support Harness deterministic diagnostic 决定。

禁止 Skill：

- 把 Quality score 升级为 Authority；
- 将 ReaderDesire 变成 Continuity Authority；
- 自行 resolve/suspend/reopen Thread；
- 自行决定 Task SATISFIED；
- 自行消费/解除 FutureReservation；
- 自行确认 integrity；
- 自行执行 REPLAN_VOLUME_ROUTE / CHANGE_REQUEST / Commit。

## 4. 控制流

SkillResult `status` 只描述本次方法调用，不控制 Narrative Runtime：

- `OK`：方法已完成；
- `NEEDS_INPUT`：Runtime 提供的 bounded context 不足；
- `NOT_APPLICABLE`：当前任务不属于本 Skill；
- `ADVISORY_ESCALATION`：建议上层 Runtime/Harness 评估扩大范围。

没有 `BLOCK` / `REPLAN` 控制权。

## 5. Formal RepairPlan 命名隔离

`RepairPlan` 是 Project Support Harness 的正式授权对象。Creative Skill 不生成 formal RepairPlan。Creative Skill 只能生成：

- `CreativeProposal`；
- `Candidate` / `CandidatePatch`；
- `QualityFinding`；
- `RepairSuggestion`；
- `EscalationAdvice`。

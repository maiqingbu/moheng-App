# 墨衡 V2.1 — Narrative Runtime 开发批次总设计 R1

**状态：FROZEN R1 MASTER PLAN**  
**输入：**
- Narrative Runtime Architecture Amendment R1
- Narrative Runtime PRD R1
- M0 frozen baseline `77f39aedc9bdb90070715f0399c6e6e1ea5eda10`

**规则：每一批必须独立验收 PASS 后才进入下一批。**

---

# 1. 总体拆分原则

批次不是“一个问题一个子系统”。

每个批次代表一条可验收的纵向能力：

```text
Authority/Contract
→ Compiler
→ Planning
→ Production
→ Volume Lifecycle
→ Creative/Quality
→ Change/Health
→ Endgame
```

永久规则：

- B01–B04 不回改；
- migration 从 0007 开始；
- CommitBundle V2 不升级；
- 新 Runtime 只有一个 production path；
- 每批必须 source-bound Evidence；
- 先 targeted，再 regression，再必要 Scale；
- 不因为测试方便重跑 70 分钟 Scale 多次。

---

# 2. Milestone 重新定义

```text
M0  = Infrastructure / Authority Foundation          COMPLETE
M1  = Narrative Runtime Core                         B05–B09
M2  = Creative Quality & Long-form Governance        B10–B11
M3  = Endgame / Completion / Product Hardening       B12+
```

---

# 3. B05 — Narrative Contracts & Authority Foundation

## 目标

把 R1 从“架构文档”变成正式、可验证的代码 Contract 和 Persistence 基础。

本批 **无 Renderer、无正式章节生成、无 Skill**。

## Scope

### B05-T00 Architecture Governance

把正式文档纳入 repo：

```text
docs/architecture/Narrative_Runtime_Architecture_Amendment_R1.md
docs/product/Narrative_Runtime_PRD_R1.md
docs/development/Narrative_Runtime_Batch_Master_Plan_R1.md
```

同时生成：

```text
ADR-026
ADR-027
ADR-028
ADR-029
ADR-030
```

Architecture gate 必须确认文档存在。

### B05-T01 Payload Schemas

第一套正式 Narrative payload schema：

```text
book-plan.r1
volume-intent.r1
volume-plan.r1
context-manifest.r1
chapter-plan.r1
chapter-exit-state.r1
volume-exit-state.r1
```

生成 TS/Rust validators/types。

### B05-T02 Migration 0007

只做 Architecture Amendment 已批准字段/索引。

禁止新增第二 authority table。

### B05-T03 Narrative Repository API

新增：

```text
repositories/narrative.rs
```

typed queries：

- Goal Contract refs；
- current relevant Canon；
- Knowledge；
- active obligations/foreshadows/mysteries；
- entity-linked dependency；
- current task states；
- Volume route；
- recent evidence。

### B05-T04 Thread Promotion Contract

实现：

```text
ThreadPromotionDecision
PROMOTE / DO_NOT_PROMOTE
```

及规则 validator。

本批可测试规则，不需要 LLM extraction。

### B05-T05 Task State Contract

冻结：

```text
NOT_STARTED
ACTIVE
SATISFIED
BLOCKED
INVALIDATED
```

以及 StateDelta value schema。


### B05-T06 Frontend Narrative Contract Foundation

新增/冻结前端 Narrative Runtime contract/type，但本批不做完整新 UI。

至少：

```text
BookPlanR1
VolumeIntentR1
VolumePlanR1
VolumeTaskStatus
ContextManifestR1
ChapterPlanR1
ContinuityThreadProjection
VolumeProgressProjection
```

要求：

```text
Frontend type
↔ Tauri DTO
↔ Rust contract
```

必须共享/受同一 contract fixture 验证。

同时加入 architecture test，禁止：

```text
mandatory Arc as production prerequisite
frontend narrative authority
deprecated static chapter timeline as production truth
```


## Blocking Acceptance

- concrete payload schema valid/invalid fixtures；
- `Plan commit != Canon mutation`；
- 0001–0006 digest/source unchanged；
- 0007 fresh/upgrade/copy-project migration PASS；
- no `continuity_threads` / `narrative_state` / `timeline_v2` tables；
- narrative SQL 只在 repositories；
- Arc optional contract；
- Frontend R1 contract/DTO/type fixtures PASS；
- UI architecture gate 禁止 mandatory Arc / frontend authority / deprecated production truth；
- pre-R1 generic payload 不得自动解释为 R1；
- all M0 regressions PASS。

## 本批禁止

```text
Plan LLM
Renderer
Audit LLM
Volume auto planning
Skill Runtime
Retcon
Endgame
```

## Exit

```text
B05 PASS
→ R1 Authority/Contract Freeze
```

---

# 4. B06 — Narrative State, Continuity & Context Compiler

## 目标

实现无 LLM 的 Narrative Planning Compiler 地基。

## Scope

### B06-T01 CurrentNarrativeStateView

从：

```text
CanonFact
CanonRelation
CanonKnowledge
CanonEvent/Timepoint
```

编译 relevant current view。

### B06-T02 ContinuityThreadView

从：

```text
NarrativeObligation
Foreshadow
Mystery
Evidence
Dependency
```

生成 logical OPEN/SUSPENDED/RESOLVED/ABANDONED view。

### B06-T03 Resume Condition Evaluator

第一批只支持明确、可测试的 condition kinds，例如：

```text
NEXT_MEETING
ENTITY_PRESENT
KNOWLEDGE_ACQUIRED
TASK_SATISFIED
AFTER_CHAPTER
IN_VOLUME
```

禁止开放任意脚本/表达式执行。

### B06-T04 VolumeProgressView

读取 VolumePlan + Task State，计算：

- active tasks；
- blocked tasks；
- remaining capacity；
- pressure；
- exit gaps。

### B06-T05 Narrative Planning Compiler

输出 ContextManifest R1。

### B06-T06 State Digest / PLAN_STALE

实现：

```text
revision fast path
+
relevant HARD digest final path
```

### B06-T07 Bounded Evidence Retrieval

通过 entity/dependency/scope/index 做 bounded recall。


### B06-T08 Frontend Projection Queries

实现最小真实查询链：

```text
UI/Application
→ FrontendPort
→ Tauri
→ Narrative Runtime
```

至少提供：

```text
CurrentNarrativeState summary
ContinuityThreadProjection
VolumeProgressProjection
ContextManifest provenance
Plan stale evaluation result
```

本批 UI 可以是 Advanced/diagnostic surface，但数据必须真实，不允许 mock 成为 production path。


## Blocking Acceptance

- Cross-volume Promise recall without LLM；
- SUSPENDED → resume condition satisfied → OPEN；
- exact evidence range reachable；
- unrelated revision not stale；
- related Canon/Knowledge/Thread/Task change stale；
- no full canon/thread scan；
- 10M fixture compiler p95 target <=300ms；
- Context HARD size bounded；
- deterministic digest repeatability；
- FrontendPort/Tauri/Rust projection chain PASS；
- frontend 不自行判断 stale/thread/task truth；
- architecture/module boundary PASS；
- M0 regression PASS。

## 本批禁止

```text
ChapterPlan LLM generation
Renderer
Audit extraction
FinalChapter production workflow
```

## Exit

```text
B06 PASS
→ Deterministic Narrative Runtime Core ready
```

---

# 5. B07 — Goal/Volume Route & Rolling Chapter Planning

## 目标

把“静态全卷章纲”正式替换为 Goal-Constrained Rolling Planning。

## Scope

### B07-T01 BookPlan SEED / APPROVED

支持：

```text
PRE_SIGNING SEED
POST_SIGNING APPROVED
```

确保黄金三章后的真实 Canon 进入 BookPlan finalization context。

### B07-T02 VolumeIntent R1

生成/修订当前卷意图。

### B07-T03 VolumePlan R1

Plan/Strategy 模型生成：

```text
Task Route
Required/Optional/Flexible/Climax/Reserved
completion predicates
capacity
exit conditions
```

不是 Chapter list。

### B07-T04 ChapterPlan R1

Plan/Strategy Model：

```text
ContextManifest
→ current ChapterPlan
+ 3–8 soft horizon
```

### B07-T05 Reservation Gate

`FUTURE_RESERVATION_VIOLATION`

### B07-T06 Task Scope Gate

`TASK_SCOPE_VIOLATION`

### B07-T07 Continuity Disposition Gate

`CONTINUITY_BRIDGE_REQUIRED`

### B07-T08 Plan-Reality Classifier Skeleton

先实现 deterministic classification interface，不需要 Final text extraction。


### B07-T09 Volume Workspace R1

同批完成用户可见规划语义切换。

Volume Workspace 至少展示：

```text
Volume Goal
Required / Optional / Flexible / Blocked Tasks
Remaining Capacity
Planning Pressure
Future Reservations
Current Executable ChapterPlan
Soft Rolling Horizon
```

要求：

- Arc 不再 mandatory；
- 不把完整静态 Chapter timeline 当 production truth；
- Rolling Horizon 明确标注 tentative；
- PLAN / REALITY / PROJECTION 视觉语义分离；
- 新 UI 必须读取 B06/B07 真实 projection/contract。


## Blocking Acceptance

- Golden three 后才创建 APPROVED BookPlan；
- VolumePlan payload 不含全量 chapters；
- no Arc volume 可以规划；
- only current ChapterPlan executable；
- horizon 不改变 Canon/Knowledge/Task；
- V5 reservation 被 V3 consume → BLOCK；
- triggered Promise missing disposition → BLOCK；
- `DEFER_WITH_BRIDGE` PASS；
- capacity pressure 会影响 planning priorities；
- Plan commit Canon unchanged；
- model call budget：规划阶段 1 call；
- Volume Workspace R1 与真实 Task Route/Capacity/Reservation/Horizon 一致；
- no mandatory Arc UI；
- no deprecated static timeline production truth；
- all B05/B06 regressions PASS。

## Exit

```text
B07 PASS
→ Planning architecture fully switched
```

---

# 6. B08 — Chapter Production Runtime

## 目标

建立第一条正式 Narrative Runtime 章节生产链。

## Scope

### B08-T01 Opening Bootstrap Production

支持黄金三章：

```text
Goal Seed + Opening Contract
→ Compiler
→ ChapterPlan
→ Renderer
→ Audit/Extraction
→ Commit
```

不要求 VolumePlan。

### B08-T02 Long-form Production

签约后：

```text
Approved Goal + Volume Route
→ same Runtime
```

### B08-T03 Renderer

ChapterPlan → ChapterCandidate。

### B08-T04 Audit + Actual Extraction

单次默认 call 输出：

- quality findings；
- canon candidates；
- knowledge candidates；
- continuity candidates；
- task candidates；
- evidence ranges。

### B08-T05 Candidate Validation

程序验证：

- Stable ID；
- evidence range；
- Canon conflict；
- Knowledge；
- Reservation；
- Thread transition；
- Task scope。

### B08-T06 Delta Compilation

Candidate → existing CommitBundle deltas。

### B08-T07 Atomic Final Commit

FinalChapter + ChapterExitState + deltas + receipt。

### B08-T08 Reconciliation

Commit 后输出：

```text
NO_ACTION
RECOMPILE_NEXT_CHAPTER
RECOMPILE_ROLLING_WINDOW
REPLAN_VOLUME_ROUTE
CHANGE_REQUEST
```


### B08-T09 Production Center R1

前端生产中心必须跟随真实生产状态：

```text
Context compiled
Plan ready
Rendering
Candidate
Auditing
Gate blocked / passed
Committed
Recompile required
```

必须可查看：

```text
current ChapterPlan
triggered Continuity
blocking Gate
evidence
ModelRun provenance
```

不得把 Candidate 显示为 FinalChapter。


## Blocking Acceptance

- Candidate 未过 Gate 不进入 FinalChapter；
- Plan expected SATISFIED but evidence only ACTIVE → reality remains ACTIVE；
- Chapter commit does not advance VolumePlan head；
- FinalChapter + delta rollback atomic；
- Cross-volume Promise exact evidence survives through production；
- flashback works without knowledge contradiction；
- NORMAL path <= 3 primary LLM calls（允许明确 fallback）；
- retry/idempotency uses B03 runtime；
- ModelRun/ContextManifest provenance complete；
- Production Center 不混淆 Candidate/Final；
- blocking Gate / stale / recompile 状态来自 authoritative Runtime；
- M0/B05/B06/B07 regressions PASS。

## Exit

```text
B08 PASS
→ First production-capable Narrative Runtime
```

---

# 7. B09 — Volume Lifecycle & Cross-Volume Rolling Production

## 目标

证明新架构不是“单章正确”，而是真的可以长期滚动到下一卷。

## Scope

### B09-T01 Planning State Projection

正式维护：

```text
planning_state_ledger_index
```

作为 rebuildable runtime projection。

### B09-T02 Capacity & Pressure

在真实章节推进中更新：

- remaining capacity；
- task pressure；
- exit gap；
- overdue continuity。

### B09-T03 Route Replan

只在正式触发条件下创建新 VolumePlan version。

### B09-T04 Volume Close

检查：

- required task；
- exit conditions；
- A/B threads；
- foreshadow；
- reservations；
- integrity。

### B09-T05 VolumeExitState R1

closure snapshot，不是唯一 memory。

### B09-T06 Next Volume

新卷 Compiler：

```text
Canon + Thread + Reservations + VolumeExitState
```

### B09-T07 Multi-volume Certification

至少构造 3–5 卷的长跨度 fixture，包括：

- V1 Promise → V3 payoff；
- V1 clue → V4 reveal；
- V2 relationship rupture → V3 bridge；
- V5 reservation protected。


### B09-T08 Cross-Volume UI

完成跨卷 Runtime 的正式 UI：

```text
Continuity panel
Volume close readiness
Open/Suspended threads
Exit-condition gaps
Next-volume handoff
Route replan reason
```

VolumeExitState 只能作为 closure snapshot 展示来源之一，UI 不得暗示其为唯一 memory。


## Blocking Acceptance

- VolumePlan 不被每章重写；
- route replan 有正式 reason/evidence；
- volume close fail-closed；
- next volume 不只依赖 VolumeExitState；
- open Thread 跨卷不丢；
- no static future ChapterPlan accumulation；
- LONG_BOOK_10M compiler/performance regression PASS；
- projection rebuild PASS；
- Cross-volume UI 与真实 Continuity/Exit/Route 状态一致；
- M0–B08 regressions PASS。

## Exit

```text
B09 PASS
→ M1 Narrative Runtime Core COMPLETE
```

---

# 8. B10 — Creative Skill & Narrative Quality Runtime

## 目标

在正确 Runtime 上再引入 Creative Skill，而不是反过来。

## Scope

- skill registry/package；
- genre/style/drama skills；
- NORMAL/IMPORTANT/CRITICAL strategy policy；
- NarrativeTasteProfile；
- ReviewReport / NarrativeQualityReport；
- ReaderDesire 作为 SOFT signal；
- repair budget；
- repetition/pacing/voice quality。

关键边界：

```text
Skill cannot mutate Authority
Quality cannot override A/B hard contract
Reader Desire cannot become Continuity Authority
```

## Blocking Acceptance

- Skill version/source provenance；
- same Runtime without Skill still works；
- NORMAL 不因 skill 数量线性增加 remote calls；
- quality repair 不破坏 Canon/Thread；
- B01–B09 regression。

---

# 9. B11 — Change / Retcon / Long-form Health

## 目标

让数百万字项目可以安全修订，而不是只会向前写。

## Scope

- ChangeRequest；
- ImpactPlan；
- typed dependency impact；
- RetconPlan；
- A/B authority decision；
- Narrative Runtime invalidation；
- targeted stateDigest stale propagation；
- Long-form health：
  - obligations aging；
  - foreshadow aging；
  - task drift；
  - character absence；
  - repeated pattern；
  - route risk。

## Blocking Acceptance

- ChangeRequest 不直接改 Canon；
- approved retcon 有 impact evidence；
- only relevant plan stale；
- old exact evidence 可追踪；
- health projection 不成为 Authority；
- B01–B10 regression。

---

# 10. B12 — Endgame, Completion & Export Hardening

## 目标

把 Runtime 从“能持续写”闭合成“能有控制地完结”。

## Scope

- EndgameContract；
- ending readiness；
- unresolved hard obligation policy；
- final future reservations；
- BookExitState；
- BookCompletionReport；
- completion integrity；
- export snapshot provenance；
- final backup/integrity workflow。

## Blocking Acceptance

- A/B unresolved obligation 不能静默完结；
- ending cannot violate endingBoundary；
- final Canon/Knowledge/Thread consistent；
- Export only committed FinalChapter；
- restore/export hash/provenance；
- full-system long-run certification。

---

# 11. 旧代码 / 旧语义处置时间表

## B05

```text
标记 deprecated/optional
禁止新增 static planner implementation
不删除 frozen schema/table
```

## B07

新 Planning path 建立后：

```text
architecture test 禁止 mandatory Arc/static ChapterSkeleton path
```

## B08

新 Chapter Production path 建立后：

```text
正式 production composition root 只能指向 Narrative Runtime
```

## B09

跨卷认证完成后：

```text
审查纯旧源码引用
```

若：

```text
production refs = 0
tests refs only = legacy compatibility
```

允许删除纯旧执行代码。

数据库表/ArtifactType 默认继续保留兼容，不做“为了干净而删 schema”。

---

# 12. Requirement → Batch Traceability

| Requirement | First implementation |
|---|---|
| FR-001 Opening Bootstrap | B08 |
| FR-002 BookPlan Finalization | B07 |
| FR-003 Goal Contract | B06/B07 |
| FR-004 Volume Route | B07 |
| FR-005 Task Reality | B05/B08 |
| FR-006 Thread Promotion | B05/B08 |
| FR-007 Exact Evidence | B06/B08 |
| FR-008 Triggered Recall | B06 |
| FR-009 Thread Disposition | B07 |
| FR-010 Reservation | B07 |
| FR-011 Current State | B06 |
| FR-012 Narrative Time | B05/B06 |
| FR-013 ContextManifest | B06 |
| FR-014 Bounded Context | B06/B09 |
| FR-015 Current Plan Only | B07 |
| FR-016 Relevant Stale | B06 |
| FR-017 Plan/Reality Separation | B05/B07 |
| FR-018 Candidate Isolation | B08 |
| FR-019 Actual Extraction | B08 |
| FR-020 Existing CommitBundle | B08 |
| FR-021 Atomic Commit | B08 |
| FR-022 Chapter Scope | B08 |
| FR-023 Reconciliation | B08 |
| FR-024 Capacity Pressure | B06/B09 |
| FR-025 Route Replan | B09 |
| FR-026 Volume Close | B09 |
| FR-027 VolumeExitState | B09 |
| FR-028 Arc Optional | B05/B07 |
| FR-029 Single Production Path | B08/B09 |
| FR-030 Legacy Compatibility | B05–B09 |
| FR-031 UI Truthfulness | B05/B07 |
| FR-032 UI Plan/Reality/Projection | B07/B08 |
| FR-033 UI Runtime Synchronization | B05–B09 |
| FR-034 UI Continuity & Evidence | B06/B09 |
| FR-035 UI Stale & Provenance | B06/B08 |

---

# 13. 每批统一 Trusted Gate 规则

每一批都必须：

```text
targeted test
→ architecture/module boundary
→ contracts
→ Rust/Node/Frontend regressions
→ dirty check
→ source commit
→ evidence sourceCommit binding
→ package smoke
→ SHA-256
```

Scale 原则：

- 有 DB/query/context 性能变化的批次才跑对应 Scale；
- 开发中不反复跑 70 分钟 LONG_BOOK；
- targeted/non-scale 全 PASS 后最终只跑 fresh certification；
- Final release evidence 禁止跨 commit reuse。

---

# 14. 冻结状态与 B05 Acceptance Basis

当前用户已确认 Architecture / PRD 方向，并追加：**Frontend UI 必须同步真实架构。**

本版已将该约束写入 Architecture、PRD 与 Batch Master Plan，三份文档正式标记 `FROZEN R1`。

B05 必须以：

```text
Frozen source baseline commit
+ Frozen Architecture SHA-256
+ Frozen PRD SHA-256
+ Frozen Batch Master Plan SHA-256
```

作为 acceptance basis。

然后进入：

```text
B05 — Narrative Contracts & Authority Foundation
```

**禁止跳过 B05 直接写 Renderer、Skill 或 Volume automation。**

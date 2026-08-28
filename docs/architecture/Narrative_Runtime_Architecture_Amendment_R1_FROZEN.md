# 墨衡 V2.1 — Narrative Runtime Architecture Amendment R1

**文档状态：FROZEN R1**  
**基线：PRE_NARRATIVE_RUNTIME_ARCHITECTURE_BASELINE**  
**Frozen Source Commit：`77f39aedc9bdb90070715f0399c6e6e1ea5eda10`**  
**M0：B01–B04 FROZEN PASS / COMPLETE**  
**适用范围：M0 之上的 Narrative Runtime、长篇规划、连续性、卷路线、章节生产。**

---

## 1. 文档权威与变更边界

本 Amendment 不重写 M0。以下能力继续冻结：

- SQLite 单一权威；
- Stable ID；
- Artifact / Canon / Plan 生命周期分离；
- CommitBundle V2；
- FinalGate；
- CommitReceipt / GateCertificate；
- Job / ModelRun / Budget / Secret / Lease；
- FTS / Backup / Restore / Deep Integrity；
- UI → Application → Port → Tauri 边界；
- Rust SQL 只能位于 `catalog/`、`commit/`、`project_db/`、`repositories/`。

本 Amendment 只替换 M0 之上的旧规划语义：

```text
旧：BookPlan → 完整 VolumePlan → ArcPlan → 静态 ChapterPlan → 正文
新：Goal Contract + Current Reality + Continuity + Volume Route
    → Narrative Planning Compiler
    → 当前章 Plan + Soft Rolling Horizon
    → 正文
    → Actual Delta
    → Existing CommitBundle
```

本 Amendment 成为 Narrative Runtime 的正式架构权威。若旧设计与本文冲突，以本文为准；M0 冻结边界仍以 B01–B04 frozen source/contracts/evidence 为准。

---

## 2. 基线源码事实

冻结源码已经存在：

- `BookPlan / VolumeIntent / VolumePlan / PlanningStateLedger / ArcPlan / ChapterPlan / ContextManifest / ChapterExitState / VolumeExitState` ArtifactType；
- `canon_facts / canon_relations / canon_knowledge / canon_events / canon_evidence / canon_timepoints`；
- `narrative_obligations / obligation_dependencies / obligation_evidence`；
- `foreshadows / foreshadow_windows / mystery_states / character_arc_states / reader_desires`；
- `state_deltas / dependency_edges`；
- `planning_state_ledger_index` projection；
- `CommitBundle` 已具有 `canonDelta / obligationDelta / foreshadowDelta / knowledgeDelta / stateDelta / dependencyEdges`；
- FinalGate 已强制单一业务 transaction scope；
- 上层 `src/domain/*`、`src/services/planning/*`、`src/production/*` 绝大多数仍为空骨架。

因此 R1 不需要第二套 Memory、Thread、Timeline、TaskGraph 或 Commit authority。

---

# 3. 架构目标

Narrative Runtime 必须同时满足：

1. 1000 万字级长篇中，跨卷承诺、关系变化、秘密、任务、伏笔不依赖模型长期记忆；
2. 计划始终服从已经正式发生的 Reality；
3. 不提前写死几十至几百章静态章纲；
4. 当前卷仍有明确 Goal、Required Task、Exit Condition、Capacity Pressure，避免无限漂移；
5. Future Reservation 能确定性阻止提前消费未来卷核心资产；
6. 每章 Context 与全书总长度解耦；
7. NORMAL 章节主远程调用目标为 3 次，允许 2–4 次预算区间；
8. 所有实际事实/状态变化必须通过现有 CommitBundle + FinalGate 原子提交；
9. 不通过“每个问题一个 Engine”扩张系统；
10. 新旧生产路径不得长期并存。

---

# 4. 核心运行模型

```text
Market / Pitch / Constitution / Opening Contract
                    │
                    ▼
         Golden Three Chapters
                    │
          Signing / Opening PASS
                    │
                    ▼
      Post-Signing Goal Finalization
                    │
                    ▼
┌───────────────────────────────────────────────┐
│ Goal Contract                                 │
│ StoryConstitution + BookPlan R1 + MainContract│
│ + VolumeIntent R1                             │
└───────────────────────────────────────────────┘
             │
             ├──────── Future Reservations
             │
             ▼
┌───────────────────────────────────────────────┐
│ VolumePlan R1 = Volume Route / Task Graph     │
└───────────────────────────────────────────────┘
             │
             ▼
┌───────────────────────────────────────────────┐
│ Narrative Planning Compiler                   │
│ + CurrentNarrativeStateView                   │
│ + ContinuityThreadView                        │
│ + VolumeProgressView                          │
└───────────────────────────────────────────────┘
             │
             ▼
      ContextManifest R1
             │
             ▼
     Plan / Strategy LLM
             │
             ▼
       ChapterPlan R1
  [current executable + soft horizon]
             │
             ▼
        Renderer LLM
             │
             ▼
       ChapterCandidate
             │
             ▼
 Audit + Actual Extraction LLM
             │
             ▼
 ActualNarrativeDeltaCandidate
             │
             ▼
      Deterministic Gates
             │
             ▼
      CommitBundle V2
             │
             ▼
FinalChapter + Canon/Knowledge/Obligation/
Foreshadow/State/Dependency + Receipt
             │
             ▼
          State(t+1)
```

---

# 5. 两阶段 BookPlan：兼容“黄金三章之后再做总纲”

## 5.1 PRE-SIGNING：BookPlan SEED

签约前允许存在 BookPlan，但其语义只是一份战略 Seed，不是“总纲”。

最低内容：

```text
planningMaturity = SEED
storyPromise
protagonistMacroDirection
endingBoundary
mustNotBreak[]
targetLengthRange
openingHypotheses[]
```

禁止包含：

- 全书章节列表；
- 完整卷章事件表；
- 未来卷具体正文事件顺序；
- 预先决定的数千章剧情节点。

Golden Three Chapters 使用：

```text
Market Strategy
+ Selected Pitch
+ StoryConstitution
+ MainContract
+ OpeningContract
+ BookPlan SEED
```

完成真实正文后形成 Canon。

## 5.2 POST-SIGNING：BookPlan APPROVED

前三章模拟签约 PASS 后，进入现有 `POST_SIGNING_PLANNING`：

```text
BookPlan SEED
+ Golden Three Actual Canon
+ Signing Evidence
→ book_plan_revise
→ BookPlan R1 planningMaturity = APPROVED
```

此时才补充：

```text
mustAchieve[]
relationshipMacroDirections[]
majorMysteryGoals[]
futureReservations[]
volumeIntentSeeds[]
globalLengthBudget
```

这才是用户意义上的“总纲”，但仍是 Goal Contract，不是静态全书章纲。

---

# 6. Authority Matrix

| Narrative Data | Authority | Projection / Cache | 禁止第二 Authority |
|---|---|---|---|
| 人物/世界事实 | CanonFact | State View / Summary | NarrativeState JSON |
| 人物关系 | CanonRelation | State View | Memory relation copy |
| 谁知道什么 | CanonKnowledge | Context View | ChapterPlan knowledge |
| 已发生事件 | CanonEvent + CanonEvidence | Timeline View | Timeline v2 DB |
| 世界时间 | CanonEvent / Timepoint | Current State View | Narrative-order field冒充时间 |
| 长期剧情责任 | NarrativeObligation / Foreshadow / Mystery | ContinuityThreadView | continuity_threads authority |
| 未来保留资产 | BookPlan/VolumeIntent payload | Compiler View | Reservation DB |
| 卷路线 | VolumePlan R1 Artifact | volume_plan_index | TaskGraph DB |
| 卷任务实际状态 | `state_deltas` current runtime state | planning_state_ledger_index | VolumePlan.status |
| 当前章节上下文 | authoritative sources | ContextManifest R1 | Memory DB |
| Chapter/Volume Exit | authoritative sources | Closure Snapshot | sole memory |

原则：

> Authority 少而明确；Runtime View 可以丰富，但 View 不得反向成为真源。

---

# 7. Goal Contract

Goal Contract 是逻辑组合，不新增 ArtifactType：

```text
StoryConstitution
+ BookPlan R1
+ MainContract
+ VolumeIntent R1
```

## 7.1 BookPlan R1

建议 payload：

```ts
interface BookPlanR1 {
  payloadSchema: "book-plan.r1"
  planningMaturity: "SEED" | "APPROVED"

  storyPromise: string
  protagonistMacroDirection: string
  endingBoundary: string

  mustAchieve: GoalConstraint[]
  mustNotBreak: GoalConstraint[]

  relationshipMacroDirections: MacroDirection[]
  majorMysteryGoals: MacroDirection[]

  futureReservations: FutureReservation[]
  volumeIntentSeeds: VolumeIntentSeed[]

  globalLengthBudget: {
    minWords?: number
    preferredWords: number
    maxWords?: number
  }
}
```

## 7.2 FutureReservation

FutureReservation 不建新表：

```ts
interface FutureReservation {
  reservationId: string
  description: string

  reservedForVolumeNo?: number
  reservedForVolumeId?: string
  notBeforeVolumeNo?: number

  prerequisites: string[]

  mayForeshadow: boolean
  mayReveal: boolean

  authorityLevel: "A" | "B"
}
```

它是 Goal Contract 的 HARD constraint。

---

# 8. VolumeIntent R1

VolumeIntent 回答：

> “这一卷为什么存在、到结束时必须变成什么样。”

```ts
interface VolumeIntentR1 {
  payloadSchema: "volume-intent.r1"

  volumeFunction: string

  entryConditions: Condition[]
  mustAchieve: GoalConstraint[]
  mustNotBreak: GoalConstraint[]
  requiredExitConditions: Condition[]

  futureReservationRefs: string[]
  crossVolumeResponsibilityRefs: string[]

  targetChapterRange: {
    min: number
    preferred: number
    max: number
  }

  targetWordRange: {
    min?: number
    preferred: number
    max?: number
  }
}
```

---

# 9. VolumePlan R1 = Volume Route / Task Graph

VolumePlan 不再保存完整章节序列。

```ts
interface VolumePlanR1 {
  payloadSchema: "volume-plan.r1"

  volumeIntentRef: ArtifactRef
  routeVersion: number

  tasks: VolumeTask[]

  routeInvariants: string[]
  requiredExitConditions: Condition[]

  climaxReservation?: {
    taskId: string
    minRemainingCapacity: number
  }

  capacityBudget: {
    minChapters: number
    preferredChapters: number
    maxChapters: number
  }
}
```

## 9.1 VolumeTask

```ts
type VolumeTaskClass =
  | "REQUIRED"
  | "OPTIONAL"
  | "FLEXIBLE"
  | "CLIMAX"
  | "RESERVED"

type TaskStatus =
  | "NOT_STARTED"
  | "ACTIVE"
  | "SATISFIED"
  | "BLOCKED"
  | "INVALIDATED"

interface VolumeTask {
  taskId: string
  class: VolumeTaskClass
  objective: string

  dependsOnTaskIds: string[]
  continuityThreadRefs: string[]
  reservationRefs: string[]

  completionPredicate: Condition

  estimatedChapterCost: {
    min?: number
    preferred: number
    max?: number
  }

  earliestPressure?: number
  latestPressure?: number
}
```

---

# 10. Task Progress：禁止伪精确百分比

叙事任务默认不使用任意 0–100%：

```text
NOT_STARTED
ACTIVE
SATISFIED
BLOCKED
INVALIDATED
```

实际状态存入现有 `state_deltas`：

```text
stateId = stable volumeTaskId
stateType = "volume.task.progress"
valueJson = {
  taskId,
  status,
  lastAdvancedChapterId,
  evidenceRefs[],
  remainingConditions[]
}
```

只有天然可量化任务允许：

```text
metric = { current: 2, target: 3 }
```

禁止：

```text
relationship_progress = 43%
worldbuilding_progress = 67%
```

---

# 11. PlanningStateLedger 的新定位

## 11.1 ArtifactType

`PlanningStateLedger` ArtifactType 在 R1 中不再作为每章推进的 mandatory canonical artifact。

状态：

```text
LEGACY_RESERVED / NO NEW REQUIRED WRITES
```

## 11.2 planning_state_ledger_index

表继续使用，并重新定义为：

> **Volume Runtime Progress Projection**

它由：

```text
VolumePlan R1
+ state_deltas
+ active obligations/foreshadows
+ committed chapters
```

确定性生成。

建议 0007 additive fields：

```text
route_plan_artifact_id
route_plan_version
task_status_json
active_task_ids_json
blocked_task_ids_json
remaining_chapter_capacity
planning_pressure_json
state_digest
generated_revision
```

不得成为 Authority。

---

# 12. Thread Promotion Policy

最重要的防膨胀规则：

> Canon 记录“发生了什么”；Continuity Thread 只表示“未来叙事必须记得处理什么”。

任何事件不能自动晋升 Thread。

## 12.1 Promotion 条件

至少满足一种：

- 明确承诺、契约、义务；
- 重大关系状态变化需要后续处理；
- 未解决冲突/目标；
- Mystery / Secret / Knowledge Reveal 责任；
- Foreshadow 必须回收；
- 关键物件/信息存在未来使用责任；
- Book/Volume Goal 显式要求；
- A/B authority 设定要求；
- FutureReservation 激活后需要持续跟踪。

## 12.2 不晋升

默认不晋升：

- 普通动作；
- 无未来影响的场景细节；
- 一次性环境描写；
- 已完成且没有后续责任的小事件；
- 只用于文风/氛围的内容。

---

# 13. ContinuityThreadView

不新增 `continuity_threads` authority table。

ThreadView 确定性组合：

```text
NarrativeObligation
Foreshadow
MysteryState
CanonEvent
CanonEvidence
DependencyEdge
```

逻辑状态：

```text
OPEN
SUSPENDED
RESOLVED
ABANDONED
```

映射规则：

- APPROVED / PLANTED / PROGRESSING → OPEN；
- resume condition 当前不满足 → SUSPENDED；
- RESOLVED / RECOVERED → RESOLVED；
- ABANDONED → ABANDONED。

不把逻辑 Thread status 再写回第二份数据库状态。

---

# 14. Resume Condition

NarrativeObligation 需要能够表达确定性 resume condition。

建议仍使用现有 `payloadJson`：

```json
{
  "resumeCondition": {
    "kind": "NEXT_MEETING",
    "entityIds": ["A", "B"]
  },
  "sourceEventId": "event:...",
  "threadClass": "PROMISE"
}
```

0007 可增加：

```text
source_event_id
resume_condition_json
```

作为高频核心字段；participant/entity relevance 通过 `dependency_edges.entity_id` 索引表达，避免额外 Thread participant 表。

---

# 15. 跨卷 Promise 示例

V1：

```text
A 对 B 说：“下次见面我一定抱你一下。”
```

正式 Chapter Commit：

```text
CanonEvent(PROMISE_MADE)
CanonEvidence(exact FinalChapter range)
NarrativeObligation(PROMISE_PAYOFF)
DependencyEdge(obligation -> A)
DependencyEdge(obligation -> B)
resumeCondition = NEXT_MEETING(A,B)
```

V2 没有见面：

```text
ThreadView = SUSPENDED
```

V3 Compiler 判断当前章可能形成 NEXT_MEETING：

```text
Thread → HARD Context
```

ChapterPlan 必须：

```text
ADVANCE
RESOLVE
DEFER_WITH_BRIDGE
NO_LONGER_APPLICABLE_CHANGE_REQUEST
```

完全忽略且发生不连续关系变化：

```text
CONTINUITY_BRIDGE_REQUIRED
```

---

# 16. CurrentNarrativeStateView

不新增 NarrativeState authority/table。

Compiler 从真实 Authority 编译：

```text
CharacterState
RelationshipState
Location / Possession
KnowledgeState
WorldTime
ActiveConflict
Relevant Open Responsibility
```

只生成当前任务需要的 bounded View。

Summary、ChapterExitState、VolumeExitState 可作为 SOFT cache，但不能覆盖 Authority。

---

# 17. Narrative Order ≠ World Time

定义：

```text
Narrative Order = chapter_no + event_ordinal
World Time      = story_time / story_time_end
```

允许：

- flashback；
- flashforward；
- 插叙；
- 同时事件。

禁止用章节顺序推导角色必然已经知道某事。

建议 0007：

```text
canon_events.event_ordinal
canon_events.story_time_end
canon_events.source_artifact_id
canon_events.source_version
```

并修复现有 CanonDelta apply 路径，使 `payloadJson.storyTime` 真正进入 `canon_events.story_time`。

---

# 18. Narrative Planning Compiler

这是 R1 唯一新增的核心 Runtime。

## 18.1 性质

```text
Deterministic
No LLM
No own DB
No own memory
No SQL outside repositories
No Canon writes
```

## 18.2 输入

HARD：

- StoryConstitution；
- BookPlan APPROVED / Opening SEED；
- MainContract；
- VolumeIntent；
- VolumePlan current route；
- current Canon/Relations/Knowledge；
- triggered A/B Continuity Threads；
- FutureReservations；
- current Task status；
- Chapter/Volume hard boundaries。

SOFT：

- recent chapter text；
- ChapterSummary / ExitState；
- ReaderDesire；
- style/taste evidence；
- retrieved historical excerpts。

## 18.3 Compiler steps

```text
1. Resolve execution mode
2. Resolve current scope
3. Load hard goal contract
4. Build relevant entity set
5. Query current canon/knowledge
6. Resolve triggered continuity threads
7. Resolve active/blocked volume tasks
8. Evaluate future reservations
9. Calculate remaining capacity / planning pressure
10. Select bounded evidence
11. Compute hard state digest
12. Emit ContextManifest R1
```

---

# 19. Rust 模块边界

建议：

```text
src-tauri/src/narrative/
  mod.rs
  state_view.rs
  continuity.rs
  progress.rs
  compiler.rs
  validation.rs
```

它们是一个 bounded module，不是多个独立 Engine。

SQL 必须留在：

```text
src-tauri/src/repositories/narrative.rs
```

或其他现有允许 SQL owner。

`narrative/*` 只能调用 typed Repository API。

TS 不再实现第二套 authoritative compiler。

---

# 20. ContextManifest R1 = ChapterExecutionContext

不新增 ArtifactType。

```ts
interface ContextManifestR1 {
  payloadSchema: "context-manifest.r1"

  executionMode: "OPENING_BOOTSTRAP" | "LONG_FORM"

  compiledFromProjectRevision: number
  compiledFromStateDigest: string

  goalContractRefs: ArtifactRef[]

  activeVolumeTaskRefs: string[]
  continuityThreadRefs: string[]
  futureReservationRefs: string[]

  narrativeStateSources: SourceRef[]
  selectedEvidence: EvidenceRef[]

  hardSourceCount: number
  omittedSoftSources: OmittedSource[]

  budget: {
    hardTokens: number
    softTokens: number
    totalTokens: number
  }
}
```

---

# 21. PLAN_STALE：Revision 快筛 + Relevant Digest 真判定

禁止：

```text
projectRevision changed
→ 无条件 PLAN_STALE
```

正式算法：

```text
if currentRevision == compiledRevision:
    VALID
else:
    recompute relevant HARD stateDigest
    if digest == compiledFromStateDigest:
        VALID
    else:
        PLAN_STALE
```

因此无关 commit 不会导致无意义重编译。

---

# 22. ChapterPlan R1

只有当前章 executable。

```ts
interface ChapterPlanR1 {
  payloadSchema: "chapter-plan.r1"

  chapterId: string

  compiledFromProjectRevision: number
  compiledFromStateDigest: string
  contextManifestRef: ArtifactRef

  chapterGoal: string
  activeTaskRefs: string[]

  continuityActions: {
    threadRef: string
    disposition:
      | "ADVANCE"
      | "RESOLVE"
      | "DEFER_WITH_BRIDGE"
      | "NO_LONGER_APPLICABLE_CHANGE_REQUEST"
    bridge?: string
  }[]

  requiredStateOutcomes: Condition[]
  forbiddenTransitions: Condition[]
  knowledgeBoundaries: string[]
  reservationRefs: string[]

  sceneExecution: unknown[]

  horizonPreview: HorizonSlot[]
}
```

---

# 23. Rolling Horizon

直接存在 ChapterPlan payload 中，不新增 ArtifactType / DB。

建议：

```text
3–8 chapters
```

未来 slot 仅允许：

- potential task；
- pressure；
- capacity reservation；
- possible conflict；
- preparation direction。

禁止未来 slot：

- 写入 Canon；
- 改 Knowledge；
- 关闭 Obligation；
- 成为 executable ChapterPlan；
- 被当成已发生事实。

---

# 24. Dynamic Planning Pressure

滚动规划不能退化为“模型自由发挥”。

Compiler 必须计算至少：

```text
remainingChapterCapacity
requiredTasksRemaining
blockedRequiredTasks
climaxReadiness
exitConditionGap
overdueThreadPressure
reservationPressure
```

Planning Pressure 是 projection，不是 Canon。

当剩余容量下降时，Required / Climax / Exit tasks 的优先级应确定性上升。

---

# 25. Future Reservation Gate

Compiler/FinalGate 必须阻止：

```text
提前兑现未来卷高潮
提前揭露 reserved secret
提前结束未来卷核心关系节点
```

错误：

```text
FUTURE_RESERVATION_VIOLATION
```

如果：

```text
mayForeshadow = true
```

则允许符合边界的 foreshadow，不允许 reveal。

---

# 26. Task Scope Gate

当前章不得推进未开放/不属于当前 Route 的强制未来任务。

错误：

```text
TASK_SCOPE_VIOLATION
```

用于阻止“为了眼前刺激偷未来剧情”。

---

# 27. Continuity Gate

Triggered Thread 必须出现正式 disposition。

以下情况 BLOCK：

- Thread 被完全遗漏；
- 角色/关系状态发生断裂式跳变；
- Promise/Obligation 被无依据视为自动完成；
- A/B authority Thread 被模型自行 abandon。

错误：

```text
CONTINUITY_BRIDGE_REQUIRED
```

---

# 28. Plan–Reality Reconciliation

不是独立 Engine。

Deterministic classification：

```text
NO_ACTION
RECOMPILE_NEXT_CHAPTER
RECOMPILE_ROLLING_WINDOW
REPLAN_VOLUME_ROUTE
CHANGE_REQUEST
```

只有：

- Volume required tasks 已无法在剩余容量完成；
- 核心 prerequisite 失效；
- Exit Conditions 变得不可达；
- 正式用户 ChangeRequest；

才触发：

```text
REPLAN_VOLUME_ROUTE
```

普通偏差只重新编译下一章。

---

# 29. ActualNarrativeDeltaCandidate

Audit LLM 只输出 Candidate：

```text
qualityFindings
canonCandidates
knowledgeCandidates
continuityCandidates
taskProgressCandidates
evidenceRanges
```

程序验证后编译到现有：

```text
CanonDelta
ObligationDelta
ForeshadowDelta
KnowledgeDelta
StateDelta
DependencyEdge
```

**不新增 `NarrativeRuntimeDelta`。**

---

# 30. 正常章节远程调用预算

NORMAL 默认：

```text
Call 1: Plan / Strategy
Call 2: Renderer
Call 3: Audit + Actual State Extraction
```

Deterministic：

```text
Context compile
validation
Final Gate
Commit
reconciliation
```

不消耗远程调用。

IMPORTANT / CRITICAL 可根据 Budget 增加：

- 额外 strategy；
- repair；
- high-confidence extraction。

禁止：

> 一个 Artifact / semantic object 自动对应一次 LLM call。

---

# 31. ChapterExitState / VolumeExitState

## ChapterExitState

保留为：

```text
derived closure snapshot / soft cache
```

可与 FinalChapter 同一 CHAPTER commit 生成。

不得作为 Canon authority。

## VolumeExitState

保留为：

```text
volume closure snapshot / next-volume fast entry
```

不再是跨卷唯一记忆。

下一卷必须仍读取：

```text
Current Canon
+ OPEN/SUSPENDED Continuity
+ Future Reservations
+ VolumeExitState
```

---

# 32. Arc 的处理

数据库 `arcs`、ArtifactType `ArcPlan / ArcContract / ArcExitState` 保留兼容。

R1：

```text
Arc = OPTIONAL TACTICAL GROUPING
```

禁止成为：

```text
Book → Volume → Arc → Chapter
```

的 mandatory runtime hop。

没有 Arc 的 Volume 也必须能完整生产。

---

# 33. 0007 Migration 建议

只允许 additive / compatibility-safe 修改。

## 33.1 narrative_obligations

建议：

```text
source_event_id TEXT
resume_condition_json TEXT NOT NULL DEFAULT '{}'
```

并继续利用：

```text
payload_json / obligation_dependencies / obligation_evidence
```

如基准证明需要，再添加专用列；禁止预先扩张。

## 33.2 canon_events

建议：

```text
event_ordinal INTEGER NOT NULL DEFAULT 0
story_time_end TEXT
source_artifact_id TEXT
source_version INTEGER
```

## 33.3 planning_state_ledger_index

建议：

```text
route_plan_artifact_id TEXT
route_plan_version INTEGER
task_status_json TEXT NOT NULL DEFAULT '{}'
active_task_ids_json TEXT NOT NULL DEFAULT '[]'
blocked_task_ids_json TEXT NOT NULL DEFAULT '[]'
remaining_chapter_capacity INTEGER
planning_pressure_json TEXT NOT NULL DEFAULT '{}'
state_digest TEXT
generated_revision INTEGER NOT NULL DEFAULT 0
```

## 33.4 Index

至少评估：

```text
dependency_edges(entity_id, dependency_type, scope)
canon_events(subject_id, event_type, chapter_id)
```

以支持 bounded relevant lookup。

---

# 34. 明确禁止新增的表 / Engine

除非后续有真实验收/性能证据，否则 R1 禁止：

```text
continuity_threads
narrative_state
timeline_events_v2
rolling_horizon
future_reservations
volume_task_graph
memory_store
checkpoint_state_v2
NarrativeRuntimeDelta
CommitBundle V3
Graph DB
Vector DB as authority
```

---

# 35. Concrete Payload Schemas

当前 baseline 的 artifact-specific schema 目录并没有正式 Narrative payload schema。

因此 R1 的：

```text
book-plan.r1
volume-intent.r1
volume-plan.r1
chapter-plan.r1
context-manifest.r1
chapter-exit-state.r1
volume-exit-state.r1
```

将是这些 Narrative Artifact 的**第一套正式具体 payload schema**，不是偷偷把一个已经正式冻结的旧 payload schema 改义。

若现场发现 persisted pre-R1 generic payload：

```text
LEGACY_UNTYPED
```

禁止自动解释为 R1；必须显式 conversion 后创建新 Artifact version。

---

# 36. Legacy / Repurpose Matrix

| Existing Component | R1 状态 | 新写入 | 旧读取 | 最终处理 |
|---|---|---:|---:|---|
| CommitBundle V2 | KEEP | YES | YES | 永久保留 |
| Canon tables | KEEP | YES | YES | 永久保留 |
| NarrativeObligation | REPURPOSE/EXTEND | YES | YES | Thread authority |
| Foreshadow/Mystery | KEEP | YES | YES | Thread input |
| BookPlan | REPURPOSE | YES R1 | YES | Goal Contract |
| VolumeIntent | REPURPOSE | YES R1 | YES | Goal Contract |
| VolumePlan | REPURPOSE | YES R1 | YES | Route/TaskGraph |
| ChapterPlan | REPURPOSE | YES R1 | YES | current executable |
| ContextManifest | REPURPOSE | YES R1 | YES | execution context |
| PlanningStateLedger Artifact | LEGACY_RESERVED | NO required writes | YES | later review |
| planning_state_ledger_index | REPURPOSE projection | YES derived | YES | runtime progress |
| ArcPlan | OPTIONAL/LEGACY | optional | YES | no mandatory path |
| ArcExitState | OPTIONAL/LEGACY | optional | YES | later review |
| ChapterExitState | REPURPOSE projection | YES | YES | soft cache |
| VolumeExitState | REPURPOSE projection | YES | YES | closure snapshot |
| old static ChapterSkeleton semantics | DEPRECATED | NO | N/A | remove references |
| TS/Rust empty planning skeletons | REUSE | YES | N/A | R1 implementation |

---

# 37. 旧代码处理原则

1. **Frozen migrations 0001–0006 永不修改。**
2. 数据库旧表不因“暂时不用”就删除。
3. 新 production path 建立后，旧 production path 禁止同时可运行。
4. 源码采用“先断流，再删除”：
   - no new writes；
   - architecture test 禁止新引用；
   - 两个后续批次确认无 production dependency；
   - 再删除纯旧实现。
5. 当前 baseline 大量目录为空，不属于需要长期兼容的旧实现。
6. UI 旧字段可阶段性兼容，但必须迁移到：
   - Required Tasks；
   - Remaining Capacity；
   - Planning Pressure；
   - Open Continuity；
   - Rolling Horizon。
7. 不允许通过 `LegacyPlanner` + `NarrativePlanner` 双栈长期共存。

---

# 38. ADR 冻结

## ADR-026 — Goal-Constrained Rolling Planning

决策：

```text
VolumePlan = Task Route
ChapterPlan = current executable
future 3–8 chapters = soft horizon
```

Supersede 静态全卷 ChapterPlan tree 的 mandatory 语义。

## ADR-027 — Continuity Thread Is a Projection

决策：

```text
NarrativeObligation/Foreshadow/Mystery/CanonEvent
= authorities
ContinuityThreadView
= runtime projection
```

必须受 Thread Promotion Policy 控制。

## ADR-028 — ExitState Is Closure Snapshot, Not Sole Memory

VolumeExitState / ChapterExitState 是快速交接投影，不替代 Canon/Thread。

## ADR-029 — Relevant State Digest Determines Plan Staleness

Project revision 是 fast path；相关 HARD state digest 是最终 stale 判定。

## ADR-030 — Single Production Path & Deferred Legacy Removal

R1 上线后只允许一个 Narrative production path。旧 schema/table 可读兼容，旧执行代码断流后再删除。

---


# 38A. UI / Frontend 是真实架构的投影，不是第二套产品语义

R1 正式冻结以下原则：

> **Frontend UI 必须反映真实 Narrative Runtime 的 Authority、Projection 与运行状态。UI 不得继续展示已经被 R1 supersede 的静态规划模型，并让用户误以为它仍是系统真相。**

## 38A.1 UI Truthfulness Contract

UI 中出现的状态必须明确来源于：

```text
Authority
或
可重建 Projection
```

禁止前端自行维护第二份：

```text
task status
continuity status
planning pressure
stale status
reservation status
chapter execution status
```

前端可以维护纯展示状态：

```text
selected tab
expanded/collapsed
sort/filter
local draft form
temporary unsaved input
```

但不得把纯前端状态升级为 Narrative Truth。

## 38A.2 R1 工作区必须逐步替换旧静态语义

旧 UI 若存在：

```text
plannedChapters
mandatory Arc hierarchy
完整静态 Chapter timeline
假百分比 progress
“计划事件 = 已发生状态”的混合展示
```

必须在对应 Runtime 能力上线时迁移。

新的 Volume Workspace 核心应围绕：

```text
Volume Goal
Required Tasks
Optional/Flexible Tasks
Blocked Tasks
Remaining Capacity
Planning Pressure
Open/Suspended Continuity Threads
Future Reservations
Current Executable ChapterPlan
Soft Rolling Horizon
```

## 38A.3 Arc UI

Arc 变为 optional tactical grouping 后：

- UI 不得要求创建 Arc 才能创建/生产 Chapter；
- 没有 Arc 的 Volume 是正常状态；
- Arc 可以作为筛选/分组视图，但不得成为强制导航层。

## 38A.4 Plan / Reality 必须视觉分离

UI 必须明确区分：

```text
PLAN
REALITY
PROJECTION
```

例如：

- `VolumePlan task objective` = PLAN；
- `task runtime status + evidence` = REALITY/derived current state；
- `planning pressure` = PROJECTION；
- `Rolling Horizon` = TENTATIVE PLAN；
- `CanonEvent` = REALITY。

禁止把 Rolling Horizon、ChapterPlan 或 FutureReservation 渲染成“已经发生”。

## 38A.5 Continuity UI

Continuity 面板必须能够展示：

```text
Thread class
logical status
source event
exact evidence link
participants/entities
resume condition
current disposition
authority level
```

但逻辑 status 来自 `ContinuityThreadView`，不得由 UI 自己修改。

用户执行“放弃/改写” A/B authority continuity 时，必须走正式：

```text
Decision / ChangeRequest
```

不能由前端 toggle 直接落库。

## 38A.6 Stale UI

当 ChapterPlan：

```text
revision changed
but stateDigest same
```

UI 不应错误显示为 stale。

只有后端返回：

```text
PLAN_STALE
```

UI 才显示需要重新编译。

## 38A.7 UI Projection Provenance

Advanced Mode 应能追踪：

```text
source artifact/version
project revision
state digest
ContextManifest
evidence refs
projection generated revision
```

普通模式只显示用户需要理解的业务含义。

## 38A.8 FrontendPort / Tauri Contract

所有新增 Narrative Runtime UI 数据必须遵守：

```text
UI
→ Application / FrontendPort
→ Tauri command
→ Rust authoritative Runtime / Repository
```

禁止：

- React/Vue component 直接访问 SQLite；
- TS 内实现第二套 Narrative Planning Compiler；
- 前端根据静态 mock 推导正式 task/thread status；
- 长期保留 mock 与 production 两条语义不同的数据路径。

## 38A.9 UI 与后端同批交付原则

如果某个 Batch 改变用户可见的正式 Narrative 语义：

> **该 Batch 必须同时完成对应最小 UI/FrontendPort/Projection 变更和验收，不允许后端标记 PASS 而 UI 仍呈现相反的旧架构。**

允许：

```text
Backend capability first within the batch
→ UI integration later in same batch
```

不允许：

```text
B07 backend PASS
→ UI 继续旧 VolumePlan 语义到 B10
```

## 38A.10 UI Architecture Gate

Architecture / module-boundary tests 必须逐步加入：

```text
no mandatory Arc UI dependency
no static full-volume ChapterPlan production path
no frontend narrative authority
no direct DB access
no deprecated field used as production truth
```

UI 是 R1 架构的一部分，不是独立于架构之外的“后续装修”。


# 39. 必须通过的架构级场景

### A. 跨卷 Promise
V1 Promise，V3 再遇 → Compiler 必须召回 exact event/evidence/thread。

### B. 合理延后
遇袭导致 Promise 无法兑现 → `DEFER_WITH_BRIDGE` PASS，Thread 保持 OPEN/SUSPENDED。

### C. Future Reservation
V5 reveal 被 V3 ChapterPlan 消费 → BLOCK。

### D. Relevant Stale
无关 commit 导致 project revision 改变但 stateDigest 不变 → Plan 仍 VALID。

### E. Real Stale
相关 Canon/Knowledge/Thread/Task 改变 → `PLAN_STALE`。

### F. Plan != Reality
Plan 预期 Task SATISFIED，实际正文仅推进 → StateDelta 必须按实际证据写 ACTIVE。

### G. Capacity Pressure
卷剩余章节很少但 Required Task 未完成 → Compiler 必须提升其 pressure。

### H. Narrative Order / World Time
倒叙合法；Knowledge 时间矛盾 BLOCK。

### I. Plan Commit Does Not Mutate Canon
Book/Volume/Chapter plan commit → Canon row/state 不变化。

### J. Chapter Commit Does Not Advance Volume Plan Head
FinalChapter CHAPTER commit → VolumePlan canonical head 不变；runtime task projection可变。

### K. 10M Bounded Context
全书扩大到 10M searchable chars → HARD context size 不线性增长；Compiler query 使用 index/scoped lookup。

### L. UI Truthfulness
后端 VolumePlan 已切换为 Task Route 后，UI 不得继续把完整静态 Chapter timeline / mandatory Arc / 假百分比进度作为 production truth；UI 必须从真实 Projection/Authority 获取 Task、Continuity、Capacity、Stale 与 Reservation 状态。

---

# 40. 架构冻结条件

正式进入 B05 开发前，必须确认：

- 本 Amendment 与 Narrative Runtime PRD R1 一致；
- 不再新增第二 Authority；
- Opening Bootstrap 与 Post-Signing Long-form 两种模式已冻结；
- Thread Promotion Policy 已冻结；
- Task Progress 离散状态模型已冻结；
- stateDigest stale 逻辑已冻结；
- Legacy/Repurpose Matrix 已冻结；
- UI Truthfulness Contract 已冻结；
- Frontend 必须与对应 Batch 的真实 Runtime 语义同步迁移；
- B05 不得越界实现 Renderer / Skill / Retcon / Endgame。

**Architecture Amendment R1 Freeze Verdict：FROZEN。**

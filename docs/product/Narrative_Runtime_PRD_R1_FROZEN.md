# 墨衡 V2.1 — Narrative Runtime PRD / Requirements R1

**文档状态：FROZEN R1**  
**依赖架构：Narrative Runtime Architecture Amendment R1**  
**基线：M0 COMPLETE / source `77f39aedc9bdb90070715f0399c6e6e1ea5eda10`**

---

# 1. 产品目的

墨衡 V2.1 的 Narrative Runtime 必须支持多百万至 1000 万+ 中文字符长篇小说持续生产，同时避免两个极端：

```text
极端 A：完全依赖 LLM 记忆 → 跨卷遗忘、人物状态跳变
极端 B：提前写死所有章纲 → 计划与真实正文越写越脱节
```

系统需要做到：

> 用确定性 Authority 保留事实与长期责任；用 Goal/Task/Reservation 保持全局方向；每章依据真实已发生状态重新规划；只把当前相关上下文交给模型。

---

# 2. 产品流程

R1 冻结流程：

```text
市场分析
→ 项目提案 / Story Direction
→ StoryConstitution / MainContract / OpeningContract
→ BookPlan SEED
→ 黄金三章
→ 模拟签约 / Opening Evaluation
→ PASS
→ BookPlan APPROVED（用户意义上的总纲）
→ 当前卷 VolumeIntent + VolumePlan Task Route
→ Rolling Chapter Production
→ Volume Close
→ Next Volume
→ Endgame / Completion
```

关键变化：

> **详细全书战略与卷路线在黄金三章真实成文后定稿，而不是在正文之前把整书章节写死。**

---

# 3. 非目标

R1 不要求：

- 自动提前生成全书所有 ChapterPlan；
- 建立第二个 Memory DB；
- 建立 Graph DB；
- 建立独立 Timeline Engine；
- 建立 Continuity Thread authority table；
- 把所有 CanonEvent 变成 Thread；
- 让模型自行决定事实能否提交；
- NORMAL 章节固定 7–8 次 LLM call；
- B05 同时完成 Renderer、Skill、Retcon、Endgame。

---

# 4. 术语

**Authority**：唯一可以决定事实/状态的持久化来源。  
**Projection/View**：可从 Authority 重建的读取模型。  
**Goal Contract**：StoryConstitution + BookPlan + MainContract + VolumeIntent。  
**Continuity Thread**：对未来叙事仍有责任的长期连续性视图。  
**Volume Route**：当前卷要完成的 TaskGraph，不是静态章纲。  
**Rolling Horizon**：未来 3–8 章软预览，仅当前章可执行。  
**Reality**：正式 FinalChapter 与经过 FinalGate 提交的 delta。  
**Plan**：未来意图，永远不能直接成为 Canon。

---

# 5. Functional Requirements

## FR-001 — Opening Bootstrap

系统必须允许黄金三章在没有正式全卷 VolumePlan 的情况下生产。

输入至少包含：

- StoryConstitution；
- MainContract；
- OpeningContract；
- BookPlan SEED；
- 当前已完成 Opening Canon。

不得要求：

- 完整全书章纲；
- 完整当前卷 TaskGraph；
- Future 50 chapters plan。

---

## FR-002 — Post-Signing BookPlan Finalization

前三章 Signing PASS 后，系统必须基于实际 opening Canon 创建新的 BookPlan APPROVED version。

新版本必须：

- 保留 story promise；
- 吸收真实开篇人物/关系/世界事实；
- 明确 mustAchieve / mustNotBreak；
- 明确 endingBoundary；
- 明确 FutureReservation；
- 产生 VolumeIntent seeds。

旧 SEED 不得被静默覆盖。

---

## FR-003 — Goal Contract Compilation

Narrative Planning Compiler 必须读取当前有效：

```text
StoryConstitution
BookPlan
MainContract
VolumeIntent
```

生成 HARD Goal Contract view。

缺失 mandatory authority：

```text
FAIL CLOSED
```

---

## FR-004 — Volume Route

每个生产中的正式卷必须拥有 VolumePlan R1。

VolumePlan 必须包含：

- required/optional/flexible/climax/reserved tasks；
- dependencies；
- completion predicates；
- capacity budget；
- required exit conditions；
- route invariants。

VolumePlan 不得保存 task actual status 作为 Plan truth。

---

## FR-005 — Task Reality

每个 Task 必须有实际 runtime status：

```text
NOT_STARTED
ACTIVE
SATISFIED
BLOCKED
INVALIDATED
```

状态必须基于正式 Chapter Reality + Evidence 更新。

叙事型任务默认禁止任意百分比。

---

## FR-006 — Continuity Promotion

Chapter Audit 发现新事件时，系统必须分别判断：

```text
Canon-worthy?
Thread-worthy?
```

Canon-worthy 不等于 Thread-worthy。

只有符合 Thread Promotion Policy 的事件，才创建/推进 NarrativeObligation/Foreshadow/Mystery continuity responsibility。

---

## FR-007 — Exact Continuity Evidence

Continuity responsibility 必须可以定位到：

```text
FinalChapter
artifact version
range
quote/evidence
source event
```

Planner 不应依赖 summary 还原关键承诺。

---

## FR-008 — Triggered Thread Recall

Compiler 必须根据：

- 当前参与实体；
- active task dependency；
- resume condition；
- deadline/aging；
- current volume；
- FutureReservation；

召回相关 OPEN/SUSPENDED Thread。

禁止：

```text
SELECT 全部 threads → 交给 LLM 自己找
```

---

## FR-009 — Thread Disposition

每个 triggered hard Thread 在 ChapterPlan 中必须有 disposition：

```text
ADVANCE
RESOLVE
DEFER_WITH_BRIDGE
NO_LONGER_APPLICABLE_CHANGE_REQUEST
```

缺失：

```text
CONTINUITY_BRIDGE_REQUIRED
```

---

## FR-010 — Future Reservation Enforcement

FutureReservation 必须是 HARD constraint。

Planner/Audit 必须区分：

```text
mayForeshadow
mayReveal
```

提前 reveal / consume：

```text
FUTURE_RESERVATION_VIOLATION
```

---

## FR-011 — Current Narrative State

系统必须从 Canon/Knowledge/Event/Runtime State 编译 bounded CurrentNarrativeStateView。

不得建立第二持久化 NarrativeState authority。

---

## FR-012 — Narrative Time

系统必须同时维护：

```text
Narrative Order
World Time
```

倒叙必须合法。

人物 Knowledge 与 World Time 冲突必须被确定性发现。

---

## FR-013 — Context Manifest

每次 Planning ModelRun 必须绑定 ContextManifest R1。

ContextManifest 必须记录：

- compiled revision；
- relevant state digest；
- Goal Contract refs；
- Task refs；
- Thread refs；
- Reservation refs；
- Evidence refs；
- HARD/SOFT budget；
- omitted soft sources。

---

## FR-014 — Bounded Context

总书规模增长时，单章 HARD Context 不得随总字数线性增长。

Context Compiler 必须使用 scoped/indexed relevant lookup。

---

## FR-015 — Current Chapter Only Executable

同一时间仅当前章 ChapterPlan 可执行。

未来 horizon slot 必须是 tentative。

Future horizon 禁止写入：

- Canon；
- Knowledge；
- Obligation lifecycle；
- task reality。

---

## FR-016 — Relevant Staleness

执行 ChapterPlan 前：

1. revision 相同 → VALID；
2. revision 不同 → 重算 relevant HARD state digest；
3. digest 相同 → VALID；
4. digest 不同 → `PLAN_STALE`。

---

## FR-017 — Plan Does Not Mutate Reality

以下 Artifact 的提交不得直接改变 Canon：

```text
BookPlan
VolumeIntent
VolumePlan
ChapterPlan
ContextManifest
```

---

## FR-018 — Candidate Isolation

Renderer 输出只能先成为 ChapterCandidate。

未通过 Audit/FinalGate：

```text
不得成为 FinalChapter
不得更新 Canon
不得完成 Thread
不得推进 Task Reality
```

---

## FR-019 — Actual Narrative Extraction

Audit 必须输出结构化 Candidate：

```text
qualityFindings
canonCandidates
knowledgeCandidates
continuityCandidates
taskProgressCandidates
evidenceRanges
```

模型无直接 DB write 权限。

---

## FR-020 — Existing CommitBundle

经程序验证后的 Candidate 必须编译成现有：

```text
CanonDelta
KnowledgeDelta
ObligationDelta
ForeshadowDelta
StateDelta
DependencyEdge
```

禁止为 R1 新增 NarrativeRuntimeDelta。

---

## FR-021 — Atomic Final Commit

FinalChapter、Narrative Reality Delta、Dependency、Receipt 必须在现有 CommitService 事务中原子提交。

任一 blocking gate FAIL：

```text
全部 rollback
```

---

## FR-022 — Chapter Scope Integrity

Chapter Final Commit 不得推进 VolumePlan canonical head。

Volume task reality 可以通过 CHAPTER commit 的 StateDelta 改变。

VolumePlan 只在显式 route replan 时创建新版本。

---

## FR-023 — Plan–Reality Reconciliation

章节 Commit 后必须输出：

```text
NO_ACTION
RECOMPILE_NEXT_CHAPTER
RECOMPILE_ROLLING_WINDOW
REPLAN_VOLUME_ROUTE
CHANGE_REQUEST
```

该分类必须确定性或规则驱动，不新增独立 LLM workflow。

---

## FR-024 — Capacity Pressure

Compiler 必须计算：

- remaining chapter capacity；
- unfinished required task；
- blocked task；
- climax readiness；
- exit condition gaps；
- continuity aging。

卷后半段仍有大量 Required Task 时，Planning Pressure 必须上升。

---

## FR-025 — Volume Route Replan

仅在以下情况允许 route replan：

- required task 在剩余容量内明显不可达；
- required exit condition 变得不可达；
- major prerequisite 失效；
- 用户 ChangeRequest；
- A/B authority 发生正式变更。

普通章节偏差不得每章重做 VolumePlan。

---

## FR-026 — Volume Close

Volume close 必须验证：

- required exit conditions；
- unresolved A/B obligations；
- required foreshadow recovery；
- forbidden future reveal；
- required task status；
- Canon integrity；
- next-volume handoff。

---

## FR-027 — VolumeExitState

VolumeExitState 必须生成 closure snapshot，但下一卷不能只读取它。

下一卷 Compiler 仍必须读取：

```text
Canon
Continuity
Reservations
VolumeExitState
```

---

## FR-028 — Arc Optionality

系统必须支持：

```text
Volume without Arc
```

Arc 仅作为可选 tactical grouping。

任何 core runtime 不得要求 arc_id 必填。

---

## FR-029 — Single Production Path

Narrative Runtime 上线后正式 production path 必须唯一。

禁止同时存在：

```text
LegacyStaticPlanner
NarrativeRuntimePlanner
```

两条可执行路径。

---

## FR-030 — Legacy Compatibility

Frozen migrations 0001–0006 不得修改。

旧表/ArtifactType 可保留读取，但停止不需要的新写入。

无 formal pre-R1 payload schema 的 Narrative Artifact 从 R1 开始建立第一套具体 payload schema。

若检测到 persisted generic pre-R1 payload：

```text
LEGACY_UNTYPED
```

必须显式 conversion，不得自动重新解释。

---


## FR-031 — UI Truthfulness

Frontend UI 必须展示真实 Narrative Runtime 的 Authority/Projection 语义。

禁止：

```text
后端已使用 Volume Task Route
但 UI 仍把完整静态 Chapter timeline 当 production truth

Arc 已 optional
但 UI 仍强制 Arc 才能创建 Chapter

Task 使用离散状态
但 UI 自行显示任意叙事百分比作为 truth

Rolling Horizon
被 UI 显示为已确定未来事实
```

UI 中所有正式状态必须来自 FrontendPort/Tauri/Rust 的 authoritative/projection query。

---

## FR-032 — UI Plan / Reality / Projection Separation

UI 必须清晰区分：

```text
PLAN
REALITY
PROJECTION
TENTATIVE
```

至少：

- Volume Task objective → PLAN；
- Current Task status/evidence → REALITY/derived runtime state；
- Planning Pressure → PROJECTION；
- Rolling Horizon → TENTATIVE；
- CanonEvent → REALITY。

不得用同一种状态视觉/字段语义混淆。

---

## FR-033 — UI Runtime Synchronization

任何开发批次改变用户可见 Narrative Runtime 语义时，同一批必须完成对应最小：

```text
FrontendPort contract
Tauri query/command
frontend projection/type
UI presentation
frontend acceptance test
```

不得以后端 PASS、前端继续旧架构作为批次 PASS。

---

## FR-034 — UI Continuity & Evidence

用户必须能够从生产 UI 查看当前相关 Continuity：

```text
status
type
source event
evidence
resume condition
authority
current disposition
```

对于 A/B authority Thread：

```text
abandon / supersede / retcon
```

必须进入正式 Decision / ChangeRequest 流程，不得由 UI 直接修改 truth。

---

## FR-035 — UI Stale & Provenance

UI 只有在 authoritative Runtime 返回 `PLAN_STALE` 时才显示 Plan stale。

Advanced Mode 必须支持查看：

```text
compiled revision
relevant state digest
ContextManifest ref
source artifact/version
evidence refs
projection revision
```

前端不得仅凭 project revision 自行判断 stale。



# 6. Model Call Requirements

## MR-001 NORMAL

目标：

```text
3 primary calls
```

1. Plan/Strategy；
2. Renderer；
3. Audit + Actual State Extraction。

## MR-002 Budget

NORMAL：

```text
2–4 remote calls
```

IMPORTANT / CRITICAL：

允许额外 strategy/repair，但必须通过已有 Budget Authority。

## MR-003 Artifact != Model Call

SceneContract、QualityReport、StateDelta 等语义对象不得自动导致独立 remote call。

---

# 7. Non-Functional Requirements

## NFR-001 — 10M Scale

在 LONG_BOOK_10M 等级：

- Compiler 不允许扫描全书 Canon/Thread；
- relevant query 使用索引/scoped lookup；
- Context HARD source count 有上限；
- total context budget 固定。

## NFR-002 — Compiler Performance

B06 冻结前建议目标：

```text
NarrativePlanningCompiler p95 <= 300 ms
```

以现有 B04 Scale fixture 做验证。

若初次实测证明阈值不合理，必须通过证据调整，不得静默放宽。

## NFR-003 — Determinism

相同：

```text
Authority snapshot
+ request
+ Compiler version
```

必须生成相同 HARD source set 与 stateDigest。

SOFT retrieval 可有差异，但必须记录 provenance。

## NFR-004 — Fail Closed

以下必须 blocking：

```text
PLAN_STALE
FUTURE_RESERVATION_VIOLATION
CONTINUITY_BRIDGE_REQUIRED
TASK_SCOPE_VIOLATION
CANON_CONFLICT
KNOWLEDGE_CONFLICT
```

## NFR-005 — Rebuildability

所有 projection：

```text
planning_state_ledger_index
ChapterExitState
VolumeExitState
UI summaries
```

不得成为不可替代真源。

## NFR-006 — Cost

无真实需求不得增加新的强制模型步骤。

新功能优先：

```text
deterministic rules
existing Authority
existing Delta
```

再考虑 remote call。

---

# 8. User-visible Requirements

R1 将 Frontend UI 视为 Narrative Runtime 的正式投影层，而不是后续装修。

卷工作区必须随对应 Runtime Batch 同步从：

```text
planned chapters / static arc timeline
```

转向展示：

```text
Volume Goal
Required Tasks
Optional Tasks
Remaining Capacity
Planning Pressure
Open Continuity Threads
Future Reservations
Rolling Horizon
```

新的正式 Volume Workspace 至少应能展示：

```text
Volume Goal
Required / Optional / Blocked Tasks
Remaining Capacity
Planning Pressure
Open / Suspended Continuity Threads
Future Reservations
Current Executable ChapterPlan
Soft Rolling Horizon
```

Advanced Mode 可展示：

- stateDigest；
- Plan version；
- ContextManifest；
- Thread evidence；
- route replan reason。

普通用户界面不显示内部 SQL/Delta/CommitBundle 技术细节。

---

# 9. Required Acceptance Stories

## AC-01 Cross-volume Promise

V1 Promise → V3 next meeting：

- Thread 被召回；
- exact evidence 可定位；
- Plan 必须 disposition；
- ignore → BLOCK。

## AC-02 Deferred Promise

V3 next meeting 遇袭：

- `DEFER_WITH_BRIDGE`；
- Thread 继续 OPEN/SUSPENDED；
- 下一符合 resume condition 的场景再次召回。

## AC-03 Reserved Secret

V5 reserved identity reveal，V3 Plan 要 reveal：

- BLOCK；
- `mayForeshadow=true` 时只允许 foreshadow。

## AC-04 Unrelated Revision

无关 commit：

- revision 改；
- relevant stateDigest 不改；
- Plan 仍 VALID。

## AC-05 Related Revision

Knowledge/Thread/Task changed：

- digest 改；
- old Plan → `PLAN_STALE`。

## AC-06 Reality Divergence

Plan 期望 SATISFIED，但正文 evidence 只支持 ACTIVE：

- actual StateDelta = ACTIVE；
- 下一章仍看到 remaining condition。

## AC-07 Capacity Drift

80–100 章卷，第 72 章尚有 required tasks：

- pressure 上升；
- Planner 不能继续无限支线。

## AC-08 No Arc

一个卷完全不创建 Arc：

- Compiler/Plan/Production 全链 PASS。

## AC-09 Flashback

Narrative order later、world time earlier：

- PASS；
- 如果 Knowledge 提前知道未来事实 → BLOCK。

## AC-10 Plan/Canon Separation

只提交 VolumePlan/ChapterPlan：

- Canon count/content unchanged。

## AC-11 Chapter Scope

FinalChapter commit：

- VolumePlan head unchanged；
- Task runtime state changes allowed。

## AC-12 10M Context Bound

从 1M 扩到 10M：

- HARD Context 不线性增长；
- p95 compiler 仍达阈值；
- 不出现全表 thread/canon scan。

## AC-13 UI Reflects Runtime

后端 VolumePlan 切换为 Task Route 后：

- UI 不再把完整静态章纲作为 production truth；
- Arc 非 mandatory；
- Task status 来自真实 Projection；
- Rolling Horizon 明确标为 tentative；
- stale 由 Runtime 决定。

## AC-14 UI Continuity Evidence

当前章触发跨卷 Promise：

- UI 能显示 Thread + exact evidence；
- 用户不能直接 toggle RESOLVED/ABANDONED；
- A/B change 必须走正式 Decision/ChangeRequest。

## AC-15 No Frontend Authority

断开/篡改前端本地 state 不得改变 Canon、Task Reality、Thread lifecycle、Reservation 或 Plan stale 结论；重新 query 后 UI 必须回到 authoritative/projection state。

---

# 10. Requirements Freeze

进入开发前必须冻结：

```text
FR-001 ~ FR-035
MR-001 ~ MR-003
NFR-001 ~ NFR-006
AC-01 ~ AC-15
```

任何开发批次如果需要违反这些要求：

```text
必须先修改 PRD / Architecture Amendment
不得在代码中偷偷改变需求。
```

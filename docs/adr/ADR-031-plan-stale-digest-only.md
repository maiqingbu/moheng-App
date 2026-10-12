# ADR-031 — Plan Staleness Uses Relevant Digest Only

状态：PROPOSED

## 背景

当前 `evaluate_plan_stale`（`src-tauri/src/narrative/runtime.rs`）忽略编译修订号，只比较已编译摘要与当前相关状态摘要。摘要绑定编译器版本和预算策略版本；因此即使 project revision 相同，版本策略变更也可能让旧计划失效。

## 与现有规格的偏离

FR-016 规定：revision 相同即可判为 `VALID`；revision 不同时才重算 relevant HARD state digest。ADR-029 则规定 revision 仅用于快筛、相关 HARD state digest 是最终判定。实现采取更严格的 digest-only 判定：每次执行前都比较摘要，不走 revision 相同的快速通道。

## 理由

只凭 revision 相同无法识别计划所绑定的编译器版本或预算策略版本已变化。相关摘要包含这些版本信息，因此摘要相同时计划仍有效，摘要不同时返回 `PLAN_STALE`，可避免静默使用旧编译规则，同时仍允许无关提交不触发过期。

## 修订建议

建议项目负责人修订 FR-016：删除“revision 相同 → VALID”的分支，改为每次执行前比较 relevant HARD state digest；digest 相同返回 `VALID`，digest 不同返回 `PLAN_STALE`。Project revision 继续作为执行快照与并发提交边界，不再作为 stale 快速放行条件。

建议同步修订 ADR-029，明确其适用于 revision 相同与不同的两种情况，并注明 relevant digest 至少绑定编译器版本和预算策略版本。

本 ADR 仅供讨论，不修改任何冻结规格或运行时代码。

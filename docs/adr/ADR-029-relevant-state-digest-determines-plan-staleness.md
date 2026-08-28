# ADR-029 — Relevant State Digest Determines Plan Staleness

状态：FROZEN R1

Project revision 只用于快筛；revision 变化后必须重算相关 HARD state digest。digest 不变时计划仍有效，digest 变化时返回 `PLAN_STALE`。

依据：Narrative Runtime Architecture Amendment R1 §21。

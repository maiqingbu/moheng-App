# ADR-026 — Goal-Constrained Rolling Planning

状态：FROZEN R1

`VolumePlan` 是任务路线，当前 `ChapterPlan` 是唯一可执行计划；未来 3–8 章只能作为 tentative horizon，不得写入 Canon 或 runtime reality。

依据：Narrative Runtime Architecture Amendment R1 §38。

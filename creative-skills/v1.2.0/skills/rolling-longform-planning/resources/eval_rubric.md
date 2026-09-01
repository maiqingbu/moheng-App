> v1.2：本 rubric 只评 Creative 方法质量与边界。HARD correctness 由 deterministic Runtime gate 单独评测；Skill 不得因本 rubric 的高分覆盖 HARD 失败。

# 长篇滚动规划：Eval Rubric

## 方法忠实度
- 每条 source 规则都能解析到 `provenance/source_map.jsonl`。
- `project_method` / `project_policy` / `model_inference` 不得标成用户来源结论。
- 不得把 v1.0 机械 source→payload 映射当成有效方法规则。
- 当前来源覆盖为 `HIGH`；输出置信度不得无证据上调。

## 结构化输出
- payload 必须包含 `planning_window`。
- payload 必须包含 `chapter_intents`。
- payload 必须包含 `state_targets`。
- payload 必须包含 `risk_watch`。
- payload 必须包含 `replan_triggers`。
- envelope 必须通过 schema；未知字段只能进入 payload 的明确扩展位置。

## 边界安全
- 不得预生成 4000 章
- 第100/1000/3000章上下文不随历史线性增长
- Canon 优先于 Summary
- 任何 HARD/Canon/Knowledge 冲突优先于文学质量建议。
- 不得读取未来不该暴露给当前 POV/Renderer 的信息。
- 不得把 Skill 自身知识当成项目事实。
- authority violation = 0；boundary fail-open = 0。

## 任务效果
- 结论能给出最小下一步或修复范围，不只给空泛分数。
- Finding 必须含证据定位；建议不得覆盖 HARD 失败。

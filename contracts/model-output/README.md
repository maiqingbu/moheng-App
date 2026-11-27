# contracts/model-output —— 模型输出规范

每个模型步骤一份 JSON Schema（Draft 2020-12），说明模型回复的形状（LD-1，见 `docs/development/LEDGER_DOWNGRADE_DESIGN.md`）。

同一份规范用于：

1. 请求：附在系统提示里，告诉模型要返回什么形状；
2. 检查：模型回复先按它校验，不合规范的回复带着校验错误重新生成一次。

规范只写后续代码真正依赖的字段，不写额外限制。新增或修改模型步骤时，先改这里。

| 文件 | 模型步骤 |
|---|---|
| `chapter-body.schema.json` | RENDER_CHAPTER_CRAFT、RENDER_CHAPTER |
| `chapter-length-adjustment.schema.json` | ADJUST_CHAPTER_LENGTH，整章扩写或按正文句号编号补写 |
| `cut-chapter.schema.json` | CUT_CHAPTER |
| `rewrite-passages.schema.json` | REWRITE_PASSAGES |
| `audit-extract.schema.json` | AUDIT_EXTRACT |
| `check-continuity.schema.json` | CHECK_CONTINUITY |
| `main-event-selection.schema.json` | SELECT_MAIN_EVENT（从已编号正文引用主要事件的结果） |
| `verify-stage-step.schema.json` | VERIFY_STAGE_STEP（引用本章正文核查新进展，可结合已验证的前章证据完成小步） |
| `confirm-narrative-continuity.schema.json` | CONFIRM_NARRATIVE_CONTINUITY（为改写后仍有的情节重演或状态倒退确认两段原文证据） |
| `check-book-continuation.schema.json` | CHECK_BOOK_CONTINUATION（核对卷一方向是否重排正式开篇已经实现的情节） |
| `confirm-book-continuation.schema.json` | CONFIRM_BOOK_CONTINUATION（比较已绑定的原文与计划，返回确认重演的候选编号） |
| `story-bible.schema.json` | GENERATE_STORY_BIBLE（story-bible.r1 合同内容字段的手写副本，另加风格描述不准带引号；`tests/ld1_model_output.rs` 检查它和合同一致。合同生成器归另一会话，暂不改它） |
| `story-directions.schema.json` | GENERATE_STORY_DIRECTIONS |

签约评估（OPENING_SIGNING_EVALUATION）沿用 S35 输出信封。模型使用 `chapterNo` 和 `sentence` 引用输入中已编号的正文，由运行时从已提交正文回填逐字引文及字符范围。

没有规范的模型步骤要写进 `src-tauri/src/model_output.rs` 测试里的豁免名单，并写明理由；新增步骤既没有规范、也不在名单里时，测试会失败。

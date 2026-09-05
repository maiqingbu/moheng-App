# B14 终局与完结治理策略 R1

## Authority boundary

- 完结状态只由 `B14::EndgameCompletionOwner` 根据 SQLite 正式记录计算。
- LLM、全文扫描、页面展示和软性建议不能直接改变完结真值。
- `BookExitState` 是关闭时的快照，不是唯一真相；已完成判定必须同时看到 `BookCompletionReport` 与 B14 DomainReceipt。
- 合同创建和全书完结都只能通过 `CommitService` 的 `CommitBundle` 写入。

## Deterministic checks

每次检查固定返回六类结果：`OBLIGATION`、`MYSTERY`、`PAYOFF`、`REQUESTED_ENDING`、`CHANGE_STALE`、`EXPORT_INTEGRITY`。

- A/B 级未完成责任、关键连续性、必需任务、相关变更和过期计划是硬阻断。
- 未映射的重要谜团需要用户决定，不自动判定完成。
- 用户明确确认的结局边界必须先编译为 `EndgameContract`，合同修订号和相关状态摘要任一过期都必须重新编译。
- S44/S45 类建议只作为 advisory，不会单独阻断 `READY`。
- 正式记录不可读时不允许返回 `READY`。

## Atomic completion

`complete_book` 在一个 `BEGIN IMMEDIATE` 事务内完成：

1. 重新读取当前正式状态并校验 expected revision；
2. 校验 EndgameContract、最终卷退出状态和六项 Gate；
3. 写入 `BookExitState` 与 `BookCompletionReport`；
4. 写入 `B14_BOOK_COMPLETION` DomainReceipt；
5. 同时推进两个项目级 canonical head。

任何一步失败都回滚。相同 idempotency key 只允许重放相同 bundle；已完成项目拒绝新的完结命令。


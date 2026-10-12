# ADR-032 — Project Writer Bounded Wait

状态：ACCEPTED（项目负责人批准）

## 背景

M0A 的 `ProjectManager` 对每个项目只允许一个写入者（`src-tauri/src/project_db/mod.rs`）。原实现中，申请写入权时如果已有持有者，`acquire_writer` 立即返回 `ManagerError::AlreadyOpen`（"project writer already open"）。

后台任务（依赖汇总、章节生产等）持有写入权期间，用户发出的任何写命令都会直接失败；后台任务刚写完 `COMPLETED`、还没释放写入权的那一瞬间也会失败（B12 偶发失败的根因，见 FX-CI）。英文原文还可能直接出现在界面上。

## 决定

1. **有上限的等待。** 写入者被占用时，申请方基于 `Mutex` + `Condvar` 等待持有者释放，最长 `WRITER_WAIT_TIMEOUT`（3 秒）。释放写入权时 `notify_all`。
2. **超时返回 `PROJECT_BUSY`。** 等待超时返回 `ManagerError::ProjectBusy`，对外错误码 `PROJECT_BUSY`。超时的命令在取得写入权之前就已失败，因此不产生任何写入，项目 revision 不变。
3. **防止自锁。** 记录持有者的线程 ID。同一线程在持有写入权时再次申请，立即返回原来的 `AlreadyOpen`，不等待——否则必然等到超时。
4. **结构性操作不等待。** 删除到回收站、复制、恢复备份在项目被打开时立即失败，错误码统一为 `PROJECT_BUSY`。这类操作会移动或替换整个项目目录，排在一个可能很长的写入之后执行既不安全，也不符合用户预期；用户应在后台任务结束后重新发起。
5. **前端。** `PROJECT_BUSY` 显示为"项目正在处理后台任务，请稍后再试。"，并提供"重试"按钮；重试使用新的命令 id 与幂等键重新提交同一请求。界面上不得出现 "project writer already open"（由 `verify:product-ux` 的 `project_busy_recovery` 检查）。

## 与 M0A 冻结边界的关系

- 单写入者不变式不变：任一时刻每个项目至多一个写入者；所有事实写入仍只经 `CommitService` + `CommitBundle V2`。
- 改变的只是"写入者被占用时"的等待策略与错误码；`AlreadyOpen` 仅保留给同线程重入这一编程错误场景。
- 不修改任何迁移、表结构或 `*_FROZEN.md`。

## 影响

- 普通写命令在后台任务短暂持有写入权时最多多等 3 秒，换来不再偶发失败。
- 两个不同线程互相等待对方的写入权时，最多 3 秒后以 `PROJECT_BUSY` 结束，不会永久死锁。
- 既有测试中断言结构性操作返回 `AlreadyOpen` 的两处（`tests/project_lifecycle.rs`）改为断言 `ProjectBusy`，依据本 ADR 第 4 条。

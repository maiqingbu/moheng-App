# Xiaomi API 真实桌面端测试记录

日期：2026-09-10

## 测试边界

- 测试入口：Tauri 桌面端真实设置页与作品流程。
- Provider：OpenAI Compatible。
- Endpoint：`https://api.xiaomimimo.com/v1`。
- 页面模型：`mimo-v2.5-pro`。
- API Key：仅通过桌面端临时输入并由系统凭据存储使用；本记录不保存密钥。测试结束后已删除对应 Keychain 凭据。
- 测试数据：新建测试作品《未命名作品》，不视为正式业务作品；未删除，留待后续处理方案确认。

## 真实 App 结果

| 功能 | 结果 | 证据/说明 |
| --- | --- | --- |
| Provider 设置保存 | PASS | 桌面端显示“Provider 设置已保存”。持久化设置中的 `model` 与 `modelId` 均为 `mimo-v2.5-pro`，`enabled=true`；设置文件未写入 `apiKey`。 |
| Xiaomi Provider 联通测试 | PASS | 桌面端“测试模型连接”显示“已完成”；实际请求返回成功。此前失败根因是 curl config 二次转义，已改为直接参数传递。 |
| 新建作品（故事想法入口） | PASS | 真实 App 创建测试项目并进入“规划·故事方向”。 |
| 故事方向选择 | PASS | 真实 App 选择故事方向并进入故事核心。 |
| 故事核心保存 | PASS | 真实 App 保存核心设定并进入开篇准备。 |
| 开篇准备读取与确认 | PASS | 页面显示第一章目标、前三章承诺、读者期待；确认后显示“开篇已准备完成”并生成第一章最小规划。 |
| 正式生产启动 | BLOCKED / DATA ISSUE | 生产任务曾进入 `RENDER` 后失败；当前数据库事实见下方，未将其报告为成功。 |

## 需要后续修复的项目数据问题

测试作品数据库：`prj:e59fabc9001e49f9a8caf4f6`

当前权威数据复核结果：

```text
job_id       = job:b3372c4070b74f3e4eb234bd
job_status   = FAILED
currentStage = RENDER
error_code   = MODEL_TRANSPORT_FAILED

model_run_id = mr:02822d06897112d667540be7
provider     = openai-compatible
model        = gpt-4o-mini
capability   = RENDER_CHAPTER
status       = FAILED
```

修复前发现的偏差：

1. 旧生产任务仍绑定旧模型 `gpt-4o-mini`；本轮已修复设置页提交时 `model` / `modelId` 不一致的问题，并重新保存为 `mimo-v2.5-pro`。
2. 生产任务数据库事实是 `FAILED/RENDER`，但桌面端生产页在重载后仍展示“进行中”和“开始第 1 章”，存在运行状态投影与权威任务状态不一致。
3. 本轮没有擅自修改、重放或删除该失败任务，等待后续专门修复方案。

测试作品当前仍为 0 个正式章节、0 字、0 成本；该测试数据保留，未执行删除。

## 本轮代码验证

- Node 全量回归：307/307 PASS。
- Vitest 全量回归：67/67 PASS。
- TypeScript typecheck：PASS。
- `cargo fmt --all -- --check`：PASS。
- `cargo check --all-targets`：PASS。
- `cargo clippy --all-targets --all-features -- -D warnings`：PASS。
- Vite `1450`：HTTP 200。

## 本轮修复摘要

- Provider curl 请求改为直接参数传递，避免 `curl --config -` 的 JSON 转义破坏 Xiaomi 请求体。
- Provider 测试失败不再永久停留在 submitting 状态。
- 设置页保存/测试会把可见模型同步到 `modelId`，endpoint 非空时显式启用 Provider。
- 保存 Provider 不再意外应用主题变化。

## 本轮 Xiaomi 生产恢复定向回归

以下测试均从当前源码执行，未改写上一次失败的 Job 或 ModelRun：

| Gate | 结果 | 证明 |
| --- | --- | --- |
| XIAOMI-R01 | PASS | FAILED/RENDER 查询为 `isActive=false`，时间线标记失败，可重试。 |
| XIAOMI-R02 | PASS | Transport failure 同步收口 ModelRun、Job、lease 与 `project_runtime_state=FAILED`。 |
| XIAOMI-R03 | PASS | 历史失败 ModelRun 仍保持 `gpt-4o-mini`。 |
| XIAOMI-R04 | PASS | 新 profile 解析为 `openai-compatible` + `mimo-v2.5-pro` + Xiaomi endpoint。 |
| XIAOMI-R05 | PASS | 缺失 model 的配置在创建 ModelRun 前 fail-closed，不产生默认模型。 |
| XIAOMI-R06 | PASS | Retry 创建新 Job，旧 Job 保持 FAILED，并记录 retry lineage。 |
| XIAOMI-R07 | PASS | Retry 重新检查当前 ChapterPlan；PLAN_STALE 在 Render 前阻断。 |
| XIAOMI-R08 | PASS | 关闭旧连接并由新 Manager 读取后仍显示 FAILED，不恢复为进行中。 |
| XIAOMI-R09 | PASS | deterministic production retry 生成新完成 Job、ModelRun 与正式产物。 |
| XIAOMI-R10 | PASS | Provider secret 不进入 settings JSON。 |

本轮自动回归：`b08_h03 33/33`、`p01_provider 11/11`、`xiaomi_app 7/7`、Node `307/307`、Vitest `67/67`、TypeScript typecheck、Cargo fmt、check、clippy 均 PASS。

## 真实 Xiaomi Retry 结果

修复后已重新执行真实 Retry，结果为诊断分支：

```text
newJob       = job:ec9a187a076324393f6b50b3
newJobStatus = FAILED
stage        = RENDER
error        = MODEL_PAYLOAD_INVALID
newModelRun  = mr:e1ecffafff47d30989b6dd4d
provider     = openai-compatible
model        = mimo-v2.5-pro
capability   = RENDER_CHAPTER
raw body     = whitespace-only / no usable JSON payload
```

旧失败记录保持不变：

```text
oldJob       = job:b3372c4070b74f3e4eb234bd
oldJobStatus = FAILED
oldModelRun  = mr:02822d06897112d667540be7
oldModel     = gpt-4o-mini
```

结论：`PRODUCTION_PROVIDER_BINDING = PASS`，但真实 Xiaomi Render 未完成，`FinalChapter = 0`、`ProductionReceipt = 0`。本批按正式指令进入 `DIAGNOSTIC_ONLY`，不标记 `REAL_XIAOMI_E2E_FINAL_PASS`。

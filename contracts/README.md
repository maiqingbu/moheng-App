# contracts/ —— 跨语言合同唯一真源

本目录是 TS / Rust / 持久化 / 测试之间**唯一合同真源**（M0-02）。

## 布局
- `schemas/` —— JSON Schema（Draft 2020-12）。子目录：
  - `common/` 基础 ID/时间/分页/共享子类型
  - `artifact/` ArtifactRef / ArtifactScope / ArtifactEnvelope
  - `commit/` CommitBundle / CommitReceipt
  - `command/` CommandEnvelope
  - `query/` QueryEnvelope / PageRequest / PageResult
  - `project/` 项目库与项目级 DTO
  - `production/` 生产相关 DTO（后批次）
- `fixtures/` —— `valid/` `invalid/` `golden/`（同一 fixture 下 TS 与 Rust 必须算出相同 SHA-256）
- `manifests/` —— `schema-manifest.json`（schema 注册与版本）
- `tools/` —— 生成器与校验器

## 规则
1. 进入权威存储、跨 IPC 或参与 digest 的核心 DTO 必须由 Schema 驱动。
2. 禁止在 TS 与 Rust 两侧手写"看起来一样"的同一 DTO。
3. 生成的类型放 `src/generated/contracts`（TS）与 `src-tauri/src/generated/contracts`（Rust），禁止手改。
4. 跨 Rust/TS/持久化/IPC 的嵌套类型必须闭合定义（独立 shared schema 或父 schema 的 `$defs`）。
5. 任何 dialect 调整必须 ADR，不得静默切换语义。

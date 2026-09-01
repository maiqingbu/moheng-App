# 墨衡 V2.1 — B11 开工前冻结记录

## 冻结范围

本记录将 B01–B10、R2 Alignment、P01 与 B09/H04 的已验收能力冻结为 B11 的基线。B11 只负责项目入口、市场/故事方向、项目助手产品化和中文用户界面接入，不建立第二套 Narrative Runtime、Harness、Authority 或 Production Root。

## Machine-readable 对应

- `freezeThrough`: `B10`
- `acceptedBaseline`: `2f6ebc02ad999c43c322ea23b4e9d48148baee01`
- `nextAuthorizedBatch`: `B11`
- `sourceOfTruth`: `evidence/B11/B11_PREBATCH_FREEZE.json`

## 允许重新打开历史能力的原因

1. B11 明确授权的产品入口、用户界面或会话接入需要；
2. B11 引入真实回归；
3. 发现违反 R2 冻结硬约束；
4. 用户另行签发正式变更指令。

## 禁止以此重新打开历史能力

Reviewer 偏好、样式调整、测试组织、顺手重构、工程字段展示偏好、中文化或去 AI 痕迹本身，均不能改变历史 Runtime owner、Authority、Gate 或正式语义。

## Migration

`src-tauri/migrations/0001`–`0010` 字节冻结。B11 优先复用现有 artifacts、decisions、support session/checkpoint、jobs/model runs 和 receipts；如确有持久化需求，只能追加 append-only migration，且不能建立第二 Authority。会话 transcript 只能作为 non-authoritative operational history。

## 状态

```text
FREEZED_BASELINE = PASS
NEXT_AUTHORIZED_BATCH = B11
DO_NOT_START = B12+
```

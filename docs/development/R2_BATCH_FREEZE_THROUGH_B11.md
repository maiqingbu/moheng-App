# R2 批次冻结：B11 之后启动 B12

## 冻结基线

- 冻结范围：B01、B02、B03、B04、B05、B06、H01、B07、H02、B08、H03、B09、H04、B10、B11
- 接受基线：`edadc96aba68fe89b8e1e453387eb6ffe3fd9968`
- 当前批次：B12 / Change Retcon Runtime
- 下一批次：B13（本批不启动）

## 冻结规则

已冻结批次的 migration、既有权威边界、正式生产路径、Skill Runtime、质量运行时和历史验收证据不因 B12 重新设计。若 B12 发现兼容性问题，只允许按 migration/contract compatibility 原则做最小兼容修复，并保留历史语义。

B12 只负责 Change/Retcon Runtime 及其确定性影响分析、失效传播、显式决策、CommitBundle V2 原子提交、回执、后验查询、中文历史工作区和 Project Assistant 接线。本批不启动 B13/B14/B15/RC1，也不接入 Creative Skill Runtime。

## 允许重新打开的原因

1. B12 指令明确要求的 Change/Retcon Runtime 新增能力。
2. B12 验收发现且能以兼容性原则证明的阻断缺陷。
3. 为生成本批 source-bound Evidence 所必需的最小测试/脚本变更。

## 禁止重新打开

- 改写 B01–B11 已冻结的业务语义或迁移历史；
- 用 UI、Harness、Skill 或 Repository 绕过正式 owner/CommitService；
- 提前实现 B13/B14/B15/RC1 能力；
- 以全图扫描、全量历史注入或 LLM 判断替代 B12 的确定性边界。


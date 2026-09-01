# 重规划与升级决策：缺口补强方法

> 来源类型：`project_method`。原始 67,818 字资料对此 Skill 覆盖不足；以下内容为项目方法补强，不进入 `WWC-*` provenance。

## 补强框架
重规划采用`根因层级→已尝试修复→传播半径→权威影响→依赖影响→成本→最小升级层级`决策。目标是停止无效重试，不是把任何难题都升级成重写全书。

## 强制诊断测试
- 同一失败连续重复相同 repair prompt，必须停止。
- 局部文本问题直接触发 GLOBAL Replan，范围过大。
- 涉及已提交 Canon 却在 Repair 内直接改，越权。
- 达到预算上限仍继续 ModelRun，FAIL。

## Provenance 规则
- 不为本文件生成伪造的 `WWC-*` source_id。
- 后续若找回原始资料，应新增真实 source_id，再决定是否把相应规则升级为 source_evidence。

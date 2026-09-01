# 场景契约设计：缺口补强方法

> 来源类型：`project_method`。原始 67,818 字资料对此 Skill 覆盖不足；以下内容为项目方法补强，不进入 `WWC-*` provenance。

## 补强框架
SceneContract 是 Renderer/Authoring 的边界合同：`进入状态、POV欲望、参与者、允许事实、禁止事实、知识边界、必须结果、可自由实现空间、退出状态`。

## 强制诊断测试
- required_outcomes 写成逐句剧情，过度约束。
- 没有 exit state 变化，场景可能无功能。
- allowed/forbidden facts 不清，存在泄露未来信息风险。
- 场景目标只能写‘对话/战斗’，要求改成状态变化。

## Provenance 规则
- 不为本文件生成伪造的 `WWC-*` source_id。
- 后续若找回原始资料，应新增真实 source_id，再决定是否把相应规则升级为 source_evidence。

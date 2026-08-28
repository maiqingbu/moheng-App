# ADR-030 — Single Production Path & Deferred Legacy Removal

状态：FROZEN R1

R1 只允许一个 Narrative production path。旧表和旧 ArtifactType 保留读取兼容；旧执行代码先断流、经后续批次确认无 production dependency 后再删除，不修改冻结 migration。

依据：Narrative Runtime Architecture Amendment R1 §37。

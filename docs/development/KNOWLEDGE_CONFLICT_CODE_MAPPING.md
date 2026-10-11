# Knowledge Conflict 错误码映射

本文将 PRD R1 中的抽象 Gate 码映射到运行时的具体错误码。冻结规格未修改。

## 映射

| PRD / 规则 | 运行时结果 | 条件 |
| --- | --- | --- |
| NFR-004 `KNOWLEDGE_CONFLICT` | `B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION` | 获得时间与事实时间均可解析，且获得时间早于事实发生时间。 |
| Knowledge 时间无法验证 | `B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME` | 获得时间或事实时间缺失、为 `UNKNOWN`、或无法按支持的世界时间格式解析。该情况阻断，但不声称已经确定发生语义冲突。 |
| Knowledge 可见范围无法验证 | `B08_AUDIT_INCOMPLETE_KNOWLEDGE_VISIBILITY` | 可见范围缺失或不符合当前角色、章节或公开范围。 |
| Canon 候选时间无法验证 | `B08_CANON_TIME_UNRESOLVED` | Canon 候选或现有 Canon 时间区间无法解析；这是 Canon 时间语义错误，不映射为 Knowledge 冲突。 |

`B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION` 是 NFR-004 `KNOWLEDGE_CONFLICT` 在 B08 生产审计中的具体码。该码也保留给“Knowledge 引用的事实来源章节晚于当前叙事章节”这一既有阻断条件；该条件检查章节顺序，不是世界时间比较。

## 比较规则

Knowledge 的 `acquiredStoryTime` 与事实 `storyTime` 都由 `canon_semantics::parse_world_time` 解析为同一可比较的时间点，再按时间先后比较。UTC `Z` 与数值时区偏移（如 `+08:00`、`-05:00`）按对应瞬间归一化；等价瞬间不构成冲突。系统不回退到字符串字典序，也不尝试猜测或转换不可解析的值。

该行为落实 FR-012 的 Narrative Order / World Time 分离和 AC-09 的 Flashback 规则：章节顺序较后的事件可以拥有较早的世界时间；只有角色在事实发生之前已获得相关知识时，才触发 `KNOWLEDGE_CONFLICT`。

在只读诊断 `diagnose_knowledge_event` 中，两个时间都可解析时按同一时间点比较；任一成对时间不可解析时以 `DATA_INCONSISTENCY` fail closed。

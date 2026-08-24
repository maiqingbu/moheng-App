# UI Projections

V2 前端只消费**可丢弃、可重建、按范围查询的只读投影**，Pinia 不是作品权威状态。

## 页面投影边界

项目进入时只读取 `ProjectOverviewProjection`。其余工作区进入后再按 `projectId + scope + pagination` 查询：

- `MarketStrategyProjection`
- `StoryDirectionProjection`
- `ProductionCenterProjection`
- `VolumeSummary / VolumeDetailProjection`
- `DecisionItem` 分页结果
- `ChapterListItem / ChapterProjection / VersionProjection`
- `CanonProjection`（各集合分页）
- `BookHealthProjection`
- `HistoryProjection`
- `SettingsProjection`

长篇阅读必须先取卷目录，再取当前卷章节目录，最后只读取当前章节和有限版本数据。禁止把全书正文、全部 Artifact、全部 Canon 或全部历史一次性装入 Pinia。

## 权威边界

Vue/Pinia 不能直接写作品状态、文件或数据库。所有状态变化都通过 typed Command 进入 Application/Production Core，再由 Tauri/Rust 单写者持久化。Event 只作为“需要刷新”的通知，收到 Event 后 UI 必须重新 Query 权威投影，不能把 Event payload 当最终事实。

`ProjectBundle` 仅存在于 `browser-demo` 内部，作为演示数据夹具的私有 backing data；它不是 `FrontendPort` Query，也不得被 UI Store 引用。

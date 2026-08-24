# Tauri Adapter Integration Boundary

前端只消费按 `projectId + scope + pagination` 查询的只读 Projection，并通过 typed Command Envelope 写入。浏览器演示适配器仅用于 UI/测试，不是正式权威实现。

正式适配器应覆盖 `FrontendPort`：项目概览、市场策略、故事方向、制片中心、卷列表/卷详情、决策分页、按卷章节目录、单章正文、章节版本、作品图谱、长篇健康、版本/Change/Retcon、项目设置与全文搜索。

禁止重新引入 `queryProjectBundle(projectId)`：它会让章节、版本、全部 Canon/Artifact 随项目长度一起进入 WebView/Pinia，违反 V2 长篇容量边界。

所有写操作使用 `commandId + projectId + expectedRevision + requestedAt + payload`；`accepted` 不等于 `committed`。Event 只触发重新 Query，不把 Event payload 当最终事实。正式生产写入只能由 Rust Kernel 单写者完成。

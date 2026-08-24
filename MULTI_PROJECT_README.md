# 多作品管理补丁说明

本补丁把原来的“单一当前作品工作区”升级为“作品库 → 选择作品 → 独立工作区”。

## 新增能力

- `/projects` 作品库：搜索、筛选、排序、状态概览、成本/章节/字数摘要；
- `/projects/new` 新建作品：新作品生成独立 `projectId`；
- `/projects/:id` 项目概览；
- 顶栏作品切换器：切换时按 `projectId` 重新查询 ProjectBundle；
- 复制作品：生成新 `projectId`，默认不继承正在运行的任务状态；
- 归档 / 恢复；
- 永久删除：UI 要求输入作品名确认；正式 Tauri 端必须执行项目级安全清理；
- 每部作品独立正文、图谱、决策、成本、签约状态、版本和项目设置；
- 应用设置与项目设置分离；
- 快速 A→B 切换加入请求序列保护，旧项目迟到响应不能覆盖当前项目。

## 新路由

```text
/                         → /projects
/projects                 作品库
/projects/new             新建作品
/projects/:id             项目概览
/projects/:id/directions  故事方向
/projects/:id/production  AI 制片中心
/projects/:id/decisions   决策收件箱
/projects/:id/read        成品阅读
/projects/:id/graph       作品图谱
/projects/:id/settings    当前项目设置
/settings                 应用级默认设置
```

## 新增 / 扩展 Port

```ts
queryProjectLibrary()
queryProjectBundle(projectId)
queryMarketOpportunities()
queryGlobalSettings()
```

项目管理 Command：

```text
project_create_from_idea
project_create_from_market
project_duplicate
project_archive
project_restore
project_delete
project_rename
```

故事方向重生成改为独立 `pitch_regenerate`，不再误用 `project_create_from_idea`。

## 替换方式

把补丁包内 `ai-novel-production-system/` 目录按原目录结构覆盖到上一版 V1 前端重构项目。新增文件也要一起复制。

建议覆盖后执行：

```bash
npm install
npm run verify
npm run typecheck
npm run build
```

当前生成环境已执行：架构扫描、TS/Vue script 语法解析、CSS 解析、Vue 标签平衡、相对 import 检查、核心 BrowserDemoPort strict TypeScript 编译，以及新建/切换/复制/归档/恢复/删除运行行为测试。

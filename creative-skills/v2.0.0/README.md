# 墨衡 Creative Skills v2.0.0

> 状态：47 个技能已全部重建，**尚未接入运行时**。接入（加载器、清单、按需选择器、提示词注入）将按 TDD 另行完成，届时替换 v1.2.0。
>
> 依据：负责人提供的网文写作方法原文（20 篇），以及《墨衡 Skill 包审查报告（2026-10-17）》。

## 一、和 v1.2 的区别

| | v1.2 对齐版 | v2 |
|---|---|---|
| 主体内容 | 边界条款、检索片段、通用套话 | 可执行的写作方法：何时用、怎么做、失败写法与改法、自检清单 |
| 原文经验数字 | 一律禁用 | 保留为"参考区间"，按作品和用户设置调整，不作硬规则 |
| 示例 | 合成的边界案例 | 全部自写，演示具体手法 |
| 调用方式 | 按执行模式固定挂 2 个 | 每个环节按信号选择，只注入该环节的"注入要点" |
| 覆盖不足 | 未标出 | 元数据标 `supplemented: true`，正文中补写条目标【补写】 |

## 二、版权处理

仓库是公开的，原文属于第三方网站，因此：

- 方法一律用自己的话复述，并在 `sources` 中注明文章名和小节；
- 示例全部自写，不使用原文案例；
- 不收录原文全文或大段摘录；
- 每次修改后运行重叠检查，要求与原文不存在 18 个字以上的连续相同片段。

## 三、文件格式

每个技能一个目录：`skills/<slug>/SKILL.md`。

**元数据（YAML）**

| 字段 | 含义 |
|---|---|
| `id` / `slug` / `title` / `version` | 标识 |
| `coverage` | 原文覆盖程度：HIGH / MEDIUM / LOW |
| `supplemented` | 是否包含补写内容（需负责人审定） |
| `role` | 可选：`evaluator`（评估）或 `repairer`（修复） |
| `stages` | 适用环节 |
| `triggers` | 选择信号：`always_in`（该环节常驻）、`scene_types`、`chapter_roles`、`chapter_position`、`findings`、`events` |
| `inject` | 每个环节注入哪一节正文，以及字数上限 `budget_chars` |
| `sources` | 原文出处（文章 · 小节） |

**正文各节：** 用途 / 何时使用与不适用 / 方法 / 参考区间（可选）/ 失败写法与改法 / 示例（自写）/ 自检清单 / 注入要点·<环节>。

## 四、按需选择

每个环节由选择器根据信号挑选技能，只注入对应的"注入要点"，不注入全文：

- **信号：** 当前环节、章节序号与位置（开篇、终卷）、本章角色、场景类型、上一轮审核发现的问题、事件（卷边界、伏笔到期、修复失败等）、注入预算。
- **规则：** `always_in` 命中的技能常驻；其余按触发条件打分，在预算内选取；选择结果和理由写入本章的生产记录。
- **举例：**
  - 第 2 章规划：章节规划、场景契约、重复与疲劳评估常驻；本章角色为"回报"时加回报设计。
  - 写作：戏剧化呈现、文字与语感常驻；场景 3 是战斗，加动作与对抗场景；场景 2 以对话为主，加对话。
  - 审核指出"节拍展开不足、篇幅低于下限"：重写环节注入局部修复，并附上不足的节拍清单和差额；两次仍失败则转重规划与升级处理。

## 五、需要负责人审定的补写技能

以下技能原文覆盖不足（`supplemented: true`），其中标【补写】的条目是通用方法，需要审定：

S01 作品核心设定、S03 主角设计、S04 配角与反派设计、S05 人物关系动态、S11 人物登场、S19 人物弧线、S22 悬念与谜团设计、S26 场景契约、S28 对话、S32 文字与语感、S41 重规划与升级处理、S45 完结回报审计。

S26 场景契约中的"每个节拍约 250～400 字"是根据章节篇幅换算出来的，原文没有直接给出，也需要确认。

## 六、技能目录

### 立项与人物

| ID | 技能 | 目录 | 原文覆盖 | 使用环节 | 备注 |
|---|---|---|---|---|---|
| S01 | 作品核心设定（立项） | `project-premise-design` | LOW | story_directions, story_setup | 含补写 |
| S02 | 读者承诺 | `reader-promise-design` | HIGH | story_setup, book_plan, review |  |
| S03 | 主角设计 | `protagonist-design` | LOW | story_setup, review | 含补写 |
| S04 | 配角与反派设计 | `supporting-cast-design` | LOW | story_setup, volume_plan, review | 含补写 |
| S05 | 人物关系动态 | `relationship-dynamics-design` | LOW | story_setup, volume_plan, chapter_plan, review | 含补写 |
| S06 | 世界规则设计 | `worldbuilding-rule-design` | HIGH | story_setup, chapter_plan, render, review |  |
| S07 | 核心机制与力量体系 | `core-mechanism-power-system` | HIGH | story_setup, volume_plan, review |  |
| S08 | 核心冲突与赌注 | `central-conflict-stakes` | HIGH | story_setup, book_plan, volume_plan, chapter_plan, review |  |

### 开篇

| ID | 技能 | 目录 | 原文覆盖 | 使用环节 | 备注 |
|---|---|---|---|---|---|
| S09 | 开篇策略比较 | `opening-strategy-search` | HIGH | story_directions, opening_plan |  |
| S10 | 钩子与好奇心 | `hook-curiosity-design` | HIGH | chapter_plan, render, review |  |
| S11 | 人物登场 | `character-entrance-design` | LOW | chapter_plan, render, review | 含补写 |
| S12 | 信息揭示 | `information-reveal-design` | HIGH | chapter_plan, render, review |  |
| S13 | 黄金三章 | `golden-three-chapters-design` | HIGH | chapter_plan, review |  |
| S14 | 章末钩子 | `chapter-ending-next-click-design` | HIGH | chapter_plan, render, review |  |

### 长篇架构

| ID | 技能 | 目录 | 原文覆盖 | 使用环节 | 备注 |
|---|---|---|---|---|---|
| S15 | 全书架构总成 | `story-architecture-synthesis` | HIGH | book_plan |  |
| S16 | 主线与支线设计 | `book-mainline-design` | HIGH | book_plan, volume_plan, review |  |
| S17 | 卷架构 | `volume-architecture` | HIGH | volume_plan, review |  |
| S18 | 段落（剧情单元）架构 | `arc-architecture` | HIGH | volume_plan, chapter_plan |  |
| S19 | 人物弧线 | `character-arc-design` | LOW | book_plan, volume_plan, chapter_plan, review | 含补写 |
| S20 | 升级与张力递进 | `escalation-design` | HIGH | volume_plan, chapter_plan, review |  |
| S21 | 节奏与密度 | `pacing-rhythm-design` | HIGH | chapter_plan, review, volume_plan |  |
| S22 | 悬念与谜团设计 | `mystery-suspense-design` | LOW | book_plan, volume_plan, chapter_plan, render, review | 含补写 |
| S23 | 伏笔与回收 | `foreshadow-payoff-design` | HIGH | book_plan, volume_plan, chapter_plan, render, review |  |
| S24 | 回报（爽点）设计 | `payoff-reward-design` | HIGH | volume_plan, chapter_plan, render, review |  |

### 章节与场景

| ID | 技能 | 目录 | 原文覆盖 | 使用环节 | 备注 |
|---|---|---|---|---|---|
| S25 | 章节规划（节拍级） | `chapter-plan-design` | MEDIUM | chapter_plan |  |
| S26 | 场景契约（场景卡） | `scene-contract-design` | LOW | chapter_plan, render, review | 含补写 |
| S27 | 戏剧化呈现（展示而非告知） | `dramatic-authoring-design` | HIGH | render, rewrite, review |  |
| S28 | 对话 | `dialogue-design` | LOW | render, review, rewrite | 含补写 |
| S29 | 情绪场景 | `emotional-scene-design` | HIGH | chapter_plan, render, review, rewrite |  |
| S30 | 动作与对抗场景 | `action-conflict-scene-design` | HIGH | chapter_plan, render, review |  |
| S31 | 信息交代融入 | `exposition-integration` | HIGH | render, rewrite, review |  |
| S32 | 文字与语感 | `prose-voice-design` | LOW | story_setup, render, rewrite, review | 含补写 |
| S33 | 视角与心理距离 | `pov-psychic-distance-design` | HIGH | story_setup, chapter_plan, render, review |  |
| S34 | 概述、蒙太奇与转场 | `summary-montage-transition-design` | HIGH | chapter_plan, render, review |  |

### 评估

| ID | 技能 | 目录 | 原文覆盖 | 使用环节 | 备注 |
|---|---|---|---|---|---|
| S35 | 开篇评估 | `opening-evaluator` | HIGH | review |  |
| S36 | 大纲与章纲评估 | `outline-evaluator` | HIGH | book_plan, volume_plan, chapter_plan |  |
| S37 | 章节叙事评估 | `chapter-narrative-evaluator` | HIGH | review |  |
| S38 | 主动性与阅读欲望评估 | `agency-reader-desire-evaluator` | MEDIUM | chapter_plan, review |  |
| S39 | 重复与疲劳评估 | `repetition-fatigue-evaluator` | HIGH | chapter_plan, review |  |

### 修复与长篇管理

| ID | 技能 | 目录 | 原文覆盖 | 使用环节 | 备注 |
|---|---|---|---|---|---|
| S40 | 局部修复 | `surgical-repair` | HIGH | rewrite |  |
| S41 | 重规划与升级处理 | `replan-escalation` | LOW | chapter_plan, volume_plan | 含补写 |
| S42 | 长篇滚动规划 | `rolling-longform-planning` | HIGH | volume_plan, chapter_plan |  |
| S43 | 长篇健康诊断 | `longform-health-diagnosis` | HIGH | volume_plan, review |  |
| S44 | 终局设计 | `endgame-design` | HIGH | book_plan, volume_plan, chapter_plan, review |  |
| S45 | 完结回报审计 | `completion-payoff-audit` | LOW | review, volume_plan | 含补写 |
| S46 | 修改润色（新增） | `revision-polish` | HIGH | rewrite, review |  |
| S47 | 幽默与喜剧 | `humor-comedy-design` | HIGH | story_setup, chapter_plan, render, review | v2 新增 |

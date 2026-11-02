# 墨衡待办与进度（总表）

最后更新：2026-11-03。分支 `release/c-r1-closeout-20261014`，PR maiqingbu/moheng-App#18，负责人要求“暂不合并”。

**换会话后先读这份文件。** 每做完一项，就更新本表并提交。

---

## 0. 工作规则（每次都要遵守）

- **语言**：用中文回复，每次回复都要写“九公子”。
- **开发流程**：
  - 测试先行：先写会失败的测试，再实现。
  - 一轮编码做完后，用 simplify 做一遍审查，再汇报。
- **关卡**：
  - 提交前跑 10 步关卡：`zsh "/Users/mac/Desktop/AI小说生产系统/交接-2026-10-26/tools/gates.sh" /Users/mac/Desktop/moheng-App <日志路径>`，看日志里有没有 10 个 `=== EXIT 0`。
  - 关卡会执行 `cargo clean`，所以真实验收正在跑时，不能同时跑关卡。
  - 跑关卡期间不改代码。
- **提交**：
  - 每一步单独提交、推送；只 `git add` 具体文件，不用 `git add -A`。
  - 提交说明末尾加 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`。
  - 不 force push；合并 PR 要负责人确认。
- **真实验收**（每次约 1 小时，¥1–2）：
  - 必须用下面这条完整命令，带写手模型和主模型两个参数。缺了参数，正文会改由 qwen-plus 写，只能写出六七百字。
  - 修仙题材加 `MOHENG_REAL_PREMISE=xianxia`；跑完先在日志里确认有 `REAL_MODEL_OVERRIDE` 这一行。

  ```bash
  cd /Users/mac/Desktop/moheng-App/src-tauri && MOHENG_RUN_REAL_TEN_CHAPTERS=1 MOHENG_REAL_PREMISE=xianxia MOHENG_REAL_APP_DATA_DIR="/Users/mac/Library/Application Support/com.moheng.ai-novel-production-system" MOHENG_REAL_RENDER_MODEL=deepseek-v4.1-flash MOHENG_REAL_MAIN_MODEL=qwen-plus MOHENG_CRAFT_LAB_OUT=<输出目录> cargo test --test real_provider_three_chapters configured_provider_writes_ten_chapters_and_evaluates_signing -- --ignored --nocapture
  ```
- **验收样章**：放在 `/Users/mac/Desktop/AI小说生产系统/交接-2026-10-26/验收样章/第N轮-…/`。
- **安全**：
  - 不读、不打印 API Key。
  - 不改平台签约规则：起点、晋江的 FAIL 保持不变。
  - 文风样本不发给模型，也不导出。

---

## 1. 已完成（2026-11-01 至 11-02）

| 编号 | 内容 | 提交 | 设计文档 |
|---|---|---|---|
| LD-8 | 放宽事件上限，只拦真正塞满的规划 | 80e2662 | LEDGER_DOWNGRADE_DESIGN §10 |
| VD-1a | 签约后由模型生成全书方向和第一卷的阶段 | 384cb27 | VOLUME_DIRECTION_DESIGN §3.1 |
| VD-1b | 卷意图、卷任务路线按阶段生成 | bd1f2ae | 同上 §3.2–3.3 |
| VD-1c | 每章规划能看到当前所处的阶段（`volumeDirection`） | 531ba74 | 同上 §3.4 |
| VD-1e | 防止原地打转：本章目标不能和最近几章重复；审计逐项报告任务进度；读者承诺写进规划输入 | 4487ace | 同上 §3.7 |
| VD-1d | 三条提醒（反派缺席 4 章、阶段人物没出场、阶段超时）；任务的首次完成章不再被覆盖 | 89ab460 | 同上 §3.6 |
| VD-1f | 节奏：阶段是一段过程，前期只推进一步；新增“节奏过快”提醒 | 3019996 | 同上 §3.8 |
| VD-1g | 阶段前半段不许照抄阶段目标；反派提醒措辞修正 | 1040d7e | 同上 §3.9 |
| VD-1h | 阶段拆成 3–5 小步，规划只看当前这一步 | 2ba5840 | 同上 §3.10 |
| VD-2 | 每次换卷，按上一卷的实际结果给新卷定方向 | 83c4997 | 同上 §7 |
| RP-1 | 章末收尾不重复；每章用一次的口头禅也能抓到；签约重复短语多一次改写 | 1dfd5cc、62ec5eb | REPETITION_DESIGN |
| RP-1 §5 | 两字意象由连贯检查判断（方案 A） | 132d12c | 同上 §5 |
| QC | 开篇章节的审计必须记下事实（第 19 轮签约失败的修复） | 132d12c | QUALITY_CONTROL_DESIGN 末节 |
| WM-2a | 负责人纠正：作品设置里的“写作规矩”和提醒上的“记住这条” | ff9e761 | WORK_MEMORY_DESIGN §8 |
| 小修 | 方向退回时的反馈改用方向自己的说法；写手只拿到和本章相关的纠正 | 8c3857d | — |

## 2. 进行中

- **第 21 轮（VD-1h）没跑完（2026-11-03）**：签约后生成方向，三次都没通过检查（7 个阶段、有一个是空的、章数超出），全书规划退回了模板；第 6 章又遇到服务商 `transport: unavailable`。已做 **VD-1i** 修复（VOLUME_DIRECTION_DESIGN §3.11）：先整理再检查，删空阶段，把章数缩放进范围；阶段上限放宽到 8；提示里写明章数范围。
- **第 22 轮（VD-1i）跑完（2026-11-03）**：10 章全部 PASS，字数 1900–2393，签约结果：番茄 PASS，起点、晋江 FAIL（按规则）。样章在 `验收样章/第22轮-修仙-VD1i`。发现两个问题：
  1. **方向又没生成**：三次都卡在“第 4 个阶段的 cast 里有故事圣经之外的人：执法堂长老”，全书规划又退回模板，第 4–10 章没有阶段方向，按小步推进这一项这轮没法检查。修法 **VD-1j**：先整理再检查时，把 cast 里故事圣经之外的人自动挪进 newCast。
  2. **第 1 章整章没有分段**：写手模型最后一次返回的正文里没有一个换行（前几次有 100 个左右），后面的步骤没有改动换行。修法 **LD-2**：配置了真实服务商时（和字数规则同一个开关），写出的正文超过 400 字却没有一个换行，就带着“没有分段”的原因重写一次（`chapter_craft::is_unbroken`）。原本想写进输出规范，但测试用的假网关正文都不分段，会让一批测试失败，所以没有那样做。
  - 这轮用的是 P1 之前的代码，所以事件、关系、伏笔表都是 0，21 条线索都停在 C 级“提议中”。另外，没写 description 的线索会拿引文当描述，出现了“他说。”这样的线索，P1 的 `openThreads` 会把这些线索给审计看，要留意。
- **第 23 轮（P1 + VD-1j + LD-2）第 1 章就失败（2026-11-03）**：`FOREIGN KEY constraint failed`。原因是审计把关系对象写成了称呼（“师父”“阿荠”），不是已有的人物编号，而关系表要求对象是已登记的人物。已修：关系对象必须是本章审计登记的人物或项目里已有的人物，否则只记事实、不记关系；关系类事实排在登记人物的事实之后（测试 `p1_an_opening_records_its_events_and_relations` 加了这个情况）。
- **下一步**：关卡通过、提交后，重跑修仙验收（第 24 轮），检查以下几项（分析办法参照会话记录里第 20 轮的做法：查 BookPlan 的 stages/steps、`state_deltas` 里 SATISFIED 的章号、各章的 `chapterGoal`、提醒内容）：
  1. 方向是否一次通过，阶段里有没有小步；
  2. 第 4–10 章是否按小步推进，有没有出现一章写完一个阶段；
  3. 第 8–10 章有没有原地打转；
  4. 签约、意象、口头禅；
  5. P1：`canon_events`、`canon_relations`、`foreshadows` 的条数，MYSTERY 线索有没有登记，线索有没有从“提议中”往前走。
- **如果节奏还是太快**：把 `chaptersSoFar` 和预计章数改成按“步”统计（VD-1h 审查时的建议）。

---

## 3. 待做（按负责人认可的优先顺序）

### P1　故事数据的漏洞（最大的问题）

第 20 轮写完 10 章后，事件、关系网、伏笔、谜题、读者期待这几张表都是 0 条，剧情线索 24 条全部停在“提议中”。后果：

- 高潮任务的完成条件 `CANON_EVENT_EXISTS/MYSTERY_RESOLVED` 永远不可能满足，卷无法正常收尾；
- 关系类可选任务（条件 `RELATION_STATE_MATCH`）永远完不成；
- 伏笔回收、伏笔到期提醒、长篇健康检查都没有数据；
- WM-2d 的事件时间线也没有数据来源。

**进度（2026-11-03，代码已写、测试通过，待关卡和提交）：**
- [x] 事件：审计给改变局面的事实标 `eventType`，写进 `canon_events`（测试 `p1_an_opening_records_its_events_and_relations`）。
- [x] 关系：宾语是另一实体的事实带 `relationId`（同一对人物共用一个关系，关系变了就更新这一条），写进 `canon_relations`。
- [x] 人物类型统一：`entityType` 归到 PERSON/PLACE/OBJECT/ORGANIZATION/CREATURE 五类（`production.rs::entity_kind`）。
- [x] 线索生命周期（负责人 11-02 批准改 B08 规则）：审计输入多了 `openThreads`（最近 8 条还开着的 C 级线索、伏笔、谜题）；章节可以在规划之外推进或兑现这些线索，不再报 `B08_THREAD_TRANSITION_OUTSIDE_PLAN`。线索会从 PROPOSED 变成 PROGRESSING，再变成 RESOLVED（测试 `p1_a_proposed_thread_…`）。
- [x] 伏笔：审计新开的线索如果是“埋下的暗示”（`foreshadowRecoveryResponsibility`），就登记成伏笔（PLANTED），之后推进时变成 REINFORCED，兑现时变成 RECOVERED（测试 `p1_a_hint_…`）。
- [x] 谜题：提交契约里没有谜题增量（加一个要改冻结的契约），所以改用结局检查已经认可的 `obligation_type='MYSTERY'`。全书规划批准时，方向里的每层谜题登记成 MYSTERY 线索（APPROVED），章节揭开时按上一条的规则 RESOLVE。它们不进卷路线的“跨卷责任”，因为高潮任务已经覆盖了它们（测试 `p1_the_approved_book_plan_records_each_mystery_layer`）。`mystery_states` 表仍然为 0，验收看 MYSTERY 线索的数量。
- [ ] 真实验收：修仙十章后各表的条数。
- 欠账：只有首次批准全书规划时会登记谜题；卷方向改写全书规划、变更流程改全书规划时，不会同步谜题。evidence 转 JSON 的写法在 `production.rs` 里有十来处重复，没有抽出来。

执行办法（每一步先写测试，再用一轮真实验收检查表里的条数）：
1. **事件**
   - AUDIT_EXTRACT 的提示要求：改变局面的事（谁对谁做了什么、什么被发现、什么被毁），在 `canonCandidates` 里标出 `eventType`，取值从有限列表里选，比如 `DISCOVERY`、`CONFRONTATION`、`LOSS`、`REVEAL`、`MYSTERY_RESOLVED`。
   - 查 `production.rs` 里 canon_delta 的组装代码，确认带 `eventType` 的事实会写进 `canon_events`（`commit/deltas.rs` 第 160 行已经有写入逻辑）。
   - 测试：审计返回带 eventType 的候选后，`canon_events` 里有对应记录；揭开谜题后，高潮任务的完成条件能被满足。
2. **关系**
   - 谓词是 `relationship`、`distrusts`、`trusts` 这一类、并且宾语是人物的事实，同时生成 `canon_relations` 增量（`commit/deltas.rs` 第 141 行已有写入）。
   - 在审计提示里给出关系谓词清单。
   - 测试：写完一章后，关系表里有“甲—关系—乙”；`RELATION_STATE_MATCH` 能判定通过。
3. **伏笔**
   - 新建通道：审计 `continuityCandidates` 里新开的线索（不是已有的线索），规划时标为伏笔的条目，生成新建伏笔的增量。现在 `production.rs` 约 4100 行只会更新已有伏笔（`EXISTING_AUTHORITY_UPDATE`），要补一条 `NEW` 分支，带种下章、预计回收窗口等字段。
   - 剧情线索（`narrative_obligations`）的生命周期：审计连续两章提到的线索，从 `PROPOSED` 转为确认；审计报告 RESOLVE 时，转为已兑现。
   - 测试：伏笔表有记录；第 N 章回收后状态变成 RECOVERED；伏笔到期提醒能触发。
4. **谜题**：VD-1a 生成的 `majorMysteryGoals` 在批准全书规划时写进 `mystery_states`（OPEN）；审计报告揭开时转为已解决（`repositories/endgame.rs` 第 75 行有写入逻辑可以复用）。
5. **人物类型统一**：审计里的 `entityType` 只能取 `PERSON`、`OBJECT`、`PLACE`、`ORGANIZATION`、`CREATURE`。CHARACTER 映射成 PERSON；ITEM、ARTIFACT 映射成 OBJECT；其余不认识的退回审计，或映射到最接近的一类。
6. **道具档案（可选）**：OBJECT 实体的来历、状态、用途记进工作记忆的细节；持有关系继续用 `owns_item`。
7. 验收标准：修仙十章跑完，`canon_events`、`canon_relations`、`foreshadows`、`mystery_states` 都不为 0，剧情线索有确认或兑现的状态变化。

### P2　Skill 没有接进全部环节

现在只有章规划、写正文、改写三个环节调用了 v2 skill（第 20 轮实际用到的：章规划 S09–S14、S21、S24–S26、S36、S39；写正文 S10、S11、S24、S27、S32；改写 S27、S32、S40、S46）。立项方向（story_directions）、开篇设定（story_setup）、全书规划（book_plan）、卷规划（volume_plan）四个环节一次都没有调用，S01–S08、S15–S20、S22、S23、S41–S43 等从来没被用上。新流程的审稿（review）环节也没有接 skill。

**进度（2026-11-03，代码已写、测试通过，待关卡和提交）：**
- [x] 四个环节都接上了 skill：`chapter_craft::setup_guidance(stage)` 给故事方向、故事底稿、全书方向、卷方向的模型输入加上 `craftGuidance`，四个提示词都写了“craftGuidance holds the selected story methods for this step; follow them.”（测试 `p2_the_setup_and_direction_steps_are_given_their_skills`，以及 `craft_v3_setup` 里的故事底稿测试）。
- [x] 覆盖检查：47 个 skill 里，原来有 3 个任何环节都选不中：S04 配角与反派、S07 核心机制与力量体系、S43 长篇健康诊断。原因是这几个环节不传信号。现在故事底稿带上“新主要人物、新能力”信号，卷方向带上“卷边界”信号，三个都能被选中（测试 `every_skill_can_be_selected_by_some_step`）。
- 选中的环节和 skill：故事方向 S09；故事底稿 S01、S03、S06，加上 S04、S07 等；全书方向 S02、S08、S15、S16、S19；卷方向 S08、S17、S18、S23，加上 S43 等。
- [ ] 回执里单独记录选了哪些 skill：还没做。模型输入里已经带着 `craftGuidance`，先不另记。
- [ ] 新流程审稿要不要接 skill：还没评估。

执行办法：
1. `book_direction::plan`（PLAN_BOOK_DIRECTION）的输入加上 `craftGuidance = chapter_craft::guidance("book_plan", signals)`；`plan_volume`（PLAN_VOLUME_DIRECTION）加上 `guidance("volume_plan", …)`。两个提示词都写明“craftGuidance holds the selected methods; follow them”。
2. GENERATE_STORY_BIBLE（`craft_setup.rs`）加上 `guidance("story_setup", …)`；GENERATE_STORY_DIRECTIONS（`project_entry.rs::generate_story_directions`）加上 `guidance("story_directions", …)`。
3. 回执里记下每个环节选了哪些 skill（参照现有 `craftSkills` 的写法）。
4. 测试：
   - 每个环节的模型输入里都有 `craftGuidance`，而且 skill 编号属于该环节；
   - 一个测试列出“每个 skill 至少会被某个环节选中”，对 47 个 skill 做覆盖检查；没覆盖到的列出原因。
5. 新流程审稿：先评估是否需要。现在用的是规则检查、连贯检查和改写，S37、S38 可以作为连贯检查的方法说明加进去。

### P3　项目助手按用户想法改东西

现状：修改申请走 B12 流程（CHANGE/RETCON → 负责人批准 → 由数据所属的模块执行，有影响范围和回执），但只支持全书规划、卷意图、卷任务路线、章规划这几类（`change_retcon.rs::target_class`）。故事底稿、人物、风格卡、事实、伏笔、负责人纠正都判为 UNSUPPORTED。

**进度（2026-11-03）：**
- [x] 第 1 步：“发起修改”多了一种理解结果 `REMEMBER`（以后都要遵守的写作规则），关键词兜底用“以后、记住、别再、不要再、从不、永远不”。助手不会直接执行，只在页面上给出一份纠正草稿：内容，以及关于谁或什么。用户核对、点“确认记住”后，才调用 `memory_correction_add`（测试：`assistant_intent` 的 `p3_a_lasting_rule_…`，以及 `tests/store/fx21-assistant-intent.test.ts`）。
- [ ] 第 2–4 步：还没做。

执行办法（要坚持的原则：助手只能提出申请，必须由负责人批准，由数据所属的模块执行，不能绕过系统直接写数据）：
1. **先接负责人纠正**：用户说“以后某人不要……”时，意图识别为 `REMEMBER`，生成 `memory_correction_add` 草稿，界面展示后由用户确认才执行。这一步不需要走 B12，因为纠正本来就由负责人亲手确认。
2. **故事底稿和人物设定**：新增 `target_class` 为 `SETUP_CHANGE`，所属模块是 B11::CraftSetupOwner。修改会生成新版故事底稿；已经写过的章节如果和新设定冲突，列在影响范围里，由负责人决定是否回溯修订（RETCON）。
3. **事实和伏笔**：继续走 RETCON。
4. 每一类都要测试：没有批准不执行；执行后有回执；越权的目标被拒绝。

### P4　模型列表、选择当前模型、思考开关

现状：设置页只有三个手填模型名的文本框（默认、写正文、审核）；没有模型列表；对小米、阿里两家写死关闭思考，没有思考开关和思考强度。

参考：第一个墨衡项目的 `/Users/mac/Desktop/墨衡/src/components/ModelSettings.vue`（940 行）和 `src/data/modelGenerationPresets.ts`，里面有 `/v1/models` 拉取列表、单个模型的思考开关、思考强度（high/max）、按场景的预设。

执行办法：
1. 后端新增 `provider_list_models` 命令：用已保存的 endpoint 和 key（从钥匙串读，不打印）请求 `/v1/models`，返回模型 id 列表；接口不支持时返回空列表，界面退回手填。
2. 设置里给每个步骤（规划、写正文、审核、方向）选模型，并分别设置 `think` 和 `reasoningEffort`。在 `ProviderSettings` 里新增字段（向后兼容），由 `provider.rs::request_payload` 按服务商写出对应的参数，各家写法不同：DeepSeek 是 `thinking`/`reasoning_effort`，阿里是 `enable_thinking`，小米是 `thinking.type`。
3. 默认保持现在的行为：关闭思考。开启思考要注意输出 token 预算，以及 JSON 模式是否兼容。
4. 测试：请求体按服务商带上正确的参数；设置能保存，读回来一致；列表接口失败时可以退回手填。

### P5　项目助手的 Codex harness 能力对接与验收

现状：已有只读能力清单（`harness/mod.rs`，25 个查询或诊断能力，每个都有上限）、受限执行器、4 种修复（重建投影、重建全文索引、重编章规划、重规划卷路线）。还没有按需求和设计逐条对照验收。

执行办法：等其他节点模块完成后，按需求文档里的助手章节逐条列出能力，对照现状做差距表，补齐后做验收。负责人已同意放到最后。

---

## 4. 推迟与欠账（已记录，暂不做）

- **VD-2 三项欠账**（推迟到“长篇换卷”专项验收时一起做）：
  - 方向生成失败后没有补救入口；
  - 新版全书规划绕过了常规检查；
  - 换卷时多出两次提交。

  解决办法：把方向生成挪到换卷提交之前（VOLUME_DIRECTION_DESIGN §7.2 的原设计）。
- **长篇换卷专项验收**：临时把第一卷的章数改小（比如 8 章），跑到换卷，检查 VD-2。
- **WM-2b、WM-2c、WM-2d**（WORK_MEMORY_DESIGN §8 的表格）：
  - 写入门槛：至少在两章出现才正式记住；
  - 记忆类型分级；
  - 事件时间线：依赖 P1 的事件数据，要在 P1 之后做。
- **WM-3 向量检索**：负责人决定放在项目收尾阶段，必做。
- **代价写得少**：读者承诺已经写进规划输入，第 17、20 轮开始出现“燃烧寿元”。继续观察，不够时单独治理。
- **都市悬疑题材**：从第 13 轮起只验过修仙题材；P1 完成后，用默认的都市题材跑一轮。
- **代码整理（审查提出的低优先级项）**：
  - “按阶段还是按小步”的判断有三处，合成 `stage_units`；
  - 意象和口头禅的改写意见共用一个函数；
  - 开篇审计“必须记下事实”的检查位置要写明意图；
  - 负责人纠正改一条就刷新整个项目。
- **发布清单重写**：等负责人说开始。
- **PR #18 合并**：CI 里 contracts drift-check 一直失败，这是本轮工作之前就有的问题，本地跑是通过的。合并要负责人确认。
- **阶段小结（交接文档）**：等负责人通知再写。
- 已关闭：任务进度“只保留首次完成章”（VD-1d 在审计这一层处理，负责人确认关闭）。

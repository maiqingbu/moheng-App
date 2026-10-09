import type { ProjectBundle } from '../../application/contracts'

export const demoProjectId = 'proj-moheng-demo'

export const demoBundle: ProjectBundle = {
  project: {
    id: demoProjectId,
    title: '墨色江湖',
    subtitle: '一个关于选择、代价与名字的东方悬疑武侠故事',
    platform: '番茄小说',
    genre: '东方玄幻 · 武侠悬疑',
    mode: 'DIRECTOR',
    phase: 'OPENING_PRODUCTION',
    runState: 'WAITING_DECISION',
    blockReason: 'DECISION_REQUIRED',
    signingStatus: 'NOT_STARTED',
    progress: 63,
    totalCost: 12.46,
    totalElapsed: '28 分钟',
    committedChapters: 2,
    candidateChapters: 1,
    latestReceiptId: 'RCP-20260823-0318',
    revision: 18,
    updatedAt: '2026-08-23 23:18'
  },
  market: [
    {
      id: 'market-stable', kind: '稳健型', title: '东方玄幻 · 宗门成长', direction: '弱系统、强人物关系、低门槛升级', platform: '番茄小说',
      audience: '18–30 岁男性读者，偏好快节奏成长与明确回报', demand: 88, competition: 81, homogeneityRisk: 78, saturation: 72, growth: 74, monetization: 86, newcomerDifficulty: 58,
      longFormCapacity: 92, originalitySpace: 63, aiFeasibility: 91, complianceRisk: 18, dataConfidence: 86, recommendationConfidence: 89,
      evidence: ['同类题材近 30 日开书量稳定', '成长/宗门关键词阅读信号持续较强', '长篇容量与商业化表现稳定'],
      recommendWhy: '市场容量大，结构稳定，AI 长篇生产可控，适合作为首个项目。',
      notRecommendWhy: '同质化明显，需要在人物关系和核心机制上建立差异。', source: '公开榜单 + 平台趋势样本', collectedAt: '2026-08-22', policyVersion: 'FQ-2026.08'
    },
    {
      id: 'market-growth', kind: '潜力型', title: '悬疑武侠 · 身份谜局', direction: '低武力开局、身份反转、门派秘史', platform: '番茄小说',
      audience: '18–35 岁泛悬疑读者，偏好连续谜团与强人物动机', demand: 82, competition: 62, homogeneityRisk: 48, saturation: 54, growth: 89, monetization: 78, newcomerDifficulty: 64,
      longFormCapacity: 84, originalitySpace: 86, aiFeasibility: 78, complianceRisk: 14, dataConfidence: 81, recommendationConfidence: 92,
      evidence: ['悬疑元素跨题材渗透增长', '身份谜局相关内容互动率上升', '中长篇留存表现优于单纯升级流样本'],
      recommendWhy: '增长势头好，差异空间明显，同时保留网文连续追读动力。',
      notRecommendWhy: '连续性要求更高，需要更严格的伏笔账本与角色知识控制。', source: '公开榜单 + 读者讨论样本', collectedAt: '2026-08-22', policyVersion: 'FQ-2026.08'
    },
    {
      id: 'market-diff', kind: '差异化型', title: '志怪江湖 · 民俗异闻', direction: '小镇单元谜案、民俗规则、慢热成长', platform: '番茄小说',
      audience: '20–38 岁偏文学与怪谈读者，偏好氛围、谜团与文化质感', demand: 68, competition: 43, homogeneityRisk: 32, saturation: 38, growth: 72, monetization: 65, newcomerDifficulty: 70,
      longFormCapacity: 77, originalitySpace: 94, aiFeasibility: 69, complianceRisk: 22, dataConfidence: 74, recommendationConfidence: 79,
      evidence: ['民俗怪谈讨论热度持续', '同类供给相对较少', '原创世界规则带来较高差异空间'],
      recommendWhy: '原创空间最大，容易形成品牌记忆点。',
      notRecommendWhy: '起量速度和商业确定性弱于前两项，文化细节要求较高。', source: '公开榜单 + 社区趋势样本', collectedAt: '2026-08-22', policyVersion: 'FQ-2026.08'
    }
  ],
  pitches: [
    {
      id: 'pitch-a', variant: '原始想法', name: '孤剑归途', recommended: false, platform: '番茄小说', genre: '东方玄幻 · 武侠成长', audience: '偏好少年成长与师门羁绊的男性读者',
      promise: '一个失去故乡的少年，在寒江剑派重新找到“归处”的故事。', emotion: '归属、师徒、重新相信他人', reward: '从被动求生到主动守护', protagonistDesire: '找到失踪兄长并证明自己不是灾星',
      mechanism: '以“剑意记忆”读取旧剑残留情绪', worldConflict: '宗门秩序与被抹去的旧江湖历史', longArc: '身份真相 → 门派裂变 → 江湖秩序重建', openingHook: '少年携一柄不属于自己的断剑求入山门',
      volumeGoal: '查清断剑主人，并在第一次宗门危机中完成立场选择', expectedLength: '80–120 万字', commercialEdge: '成长逻辑清晰，关系线稳定', homogeneityRisk: '传统宗门成长感偏强',
      originality: '剑意记忆与“名字被篡改”的世界线索形成差异', difficulty: '需要避免宗门日常拖慢悬疑主线', failureRisk: '前期若冲突不足，容易显得常规', aiReason: '最忠于原始“少年拜师 + 神秘断剑”的想法。'
    },
    {
      id: 'pitch-b', variant: '商业增强', name: '墨色江湖', recommended: true, platform: '番茄小说', genre: '东方玄幻 · 武侠悬疑', audience: '18–35 岁泛悬疑和成长型网文读者',
      promise: '少年发现整个江湖的名字都可能被人改写，而自己的过去是第一处被篡改的证据。', emotion: '身份焦虑、信任、选择代价', reward: '每揭开一层真相，就获得一次可见成长和一次更大的危险',
      protagonistDesire: '找回真实身份，并阻止他人继续篡改江湖记忆', mechanism: '半枚玉符可以校验“被改写的名字”，但每次使用都会暴露位置', worldConflict: '权力通过命名与记录控制“什么是真相”',
      longArc: '玉符 → 门派旧案 → 多地命名异常 → 朝廷档案体系 → 真名战争', openingHook: '第三章雨夜来客叫出了主角从未听过的真名', volumeGoal: '确认玉符与寒江剑派旧案的联系，击穿第一层身份骗局',
      expectedLength: '120–180 万字', commercialEdge: '强悬念、明确升级、每卷可独立形成谜局', homogeneityRisk: '若规则解释过多，会削弱爽感', originality: '将“命名”从装饰升级为世界权力机制',
      difficulty: '必须严格维护谁知道什么、谁在撒谎', failureRisk: '悬疑债务失控会造成后期崩塌', aiReason: '市场增长与项目差异之间最平衡，前三章钩子最强。'
    },
    {
      id: 'pitch-c', variant: '差异化', name: '山鬼夜话', recommended: false, platform: '番茄小说', genre: '志怪 · 民俗悬疑', audience: '偏好怪谈、民俗和文学质感的读者',
      promise: '少年每到一个地方，都会遇见一个“名字与现实不一致”的怪谈。', emotion: '陌生感、乡土记忆、失落与重建', reward: '解开规则谜题，恢复被遗忘的人和地方', protagonistDesire: '找到一个地图上不存在的故乡',
      mechanism: '旧纸谱记载的俗名会在夜里变成真实规则', worldConflict: '官方秩序不断覆盖民间记忆', longArc: '单元怪谈 → 地方谱系 → 故乡消失真相 → 名字与存在的最终冲突',
      openingHook: '镇上所有人都否认一条每天夜里出现的巷子', volumeGoal: '解决水墨镇异闻并找到第一条故乡线索', expectedLength: '60–100 万字', commercialEdge: '氛围和记忆点强', homogeneityRisk: '商业节奏相对慢',
      originality: '以地名、俗名和民俗规则串联长篇', difficulty: '民俗细节与单元结构需要大量设计', failureRisk: '慢热导致开篇转化不足', aiReason: '原创空间最大，但商业确定性略低。'
    }
  ],
  timeline: [
    { id: 't1', order: 1, title: '故事方向确认', description: '已确认《墨色江湖》为当前立项方向。', status: 'done', receipt: 'DEC-0007', elapsed: '3 分钟' },
    { id: 't2', order: 2, title: '故事蓝图与命名', description: '故事承诺、主角方向、主要冲突和命名体系已形成。', status: 'done', receipt: 'ART-BLUEPRINT-v2', elapsed: '4 分钟' },
    { id: 't3', order: 3, title: '故事宪法', description: '核心承诺、主角边界、世界规则与终局禁区已冻结。', status: 'done', receipt: 'ART-CONST-v3', elapsed: '4 分钟' },
    { id: 't4', order: 4, title: '全书战略与约束', description: '全书战略已建立主要承诺与各卷意图，全书约束已编译硬/软规则。', status: 'done', receipt: 'ART-BOOK-v1', elapsed: '5 分钟' },
    { id: 't5', order: 5, title: '作品图谱初始化', description: '首批人物、关系、伏笔、剧情义务和角色知识状态已建立。', status: 'done', receipt: 'RCP-GRAPH-001', elapsed: '2 分钟' },
    { id: 't6', order: 6, title: '开篇三章规则', description: '目标平台开篇义务已经编译完成。', status: 'done', receipt: 'ART-OPEN-v2', elapsed: '1 分钟' },
    { id: 't7', order: 7, title: '前三章生产', description: '前两章正式提交；第三章等待你的方向确认。', status: 'active', elapsed: '16 分钟' },
    { id: 't8', order: 8, title: '开篇评估', description: '前三章全部正式提交后启动独立模拟签约审核。', status: 'locked' },
    { id: 't9', order: 9, title: '当前卷详细规划', description: '只有开篇评估通过并生成正式回执后，才按当前卷创建详细卷计划。', status: 'locked' },
    { id: 't10', order: 10, title: '后续长篇生产', description: '按卷生产、卷关闭与退出状态滚动推进，不一次性生成整书章纲。', status: 'locked' }
  ],
  tasks: [
    { id: 'task-plan', title: '第三章 · 章节规划', description: '剧情义务和场景目标已完成。', status: 'done', cost: 0.08, elapsed: '42 秒' },
    { id: 'task-draft', title: '第三章 · 正文生成', description: '制作稿已生成，等待后续审核。', status: 'done', cost: 0.18, elapsed: '2 分 16 秒' },
    { id: 'task-rule', title: '规则与合同检查', description: '字数、禁区、开篇义务与硬约束均通过。', status: 'done', cost: 0.02, elapsed: '18 秒' },
    { id: 'task-cont', title: '人物与设定连续性', description: '发现 1 处角色动机表达偏弱，已进入剧情审核。', status: 'done', cost: 0.06, elapsed: '58 秒' },
    { id: 'task-review', title: '剧情与文学审核', description: '建议增强雨夜来客与主角的正面冲突。', status: 'active', progress: 78, cost: 0.13, elapsed: '进行中' },
    { id: 'task-repair', title: '定向修复', description: '等待你确认推荐修复方向。', status: 'waiting' },
    { id: 'task-rerun', title: '完整复检', description: '修复完成后重新执行全部审核。', status: 'waiting' },
    { id: 'task-commit', title: '正式收入作品', description: '全部质量门通过后原子提交正文与图谱增量。', status: 'waiting' }
  ],
  artifacts: [
    { id: 'art-brief', title: '创作要求', kind: 'Creative Brief', version: 'v2', status: 'official', summary: '悬疑武侠；主角主动；不要悲剧；避免后宫与低智误会。', updatedAt: '22:34' },
    { id: 'art-blueprint', title: '故事蓝图', kind: 'Story Blueprint', version: 'v2', status: 'official', summary: '冻结故事承诺、主角方向、核心机制、主要冲突和第一卷目标。', updatedAt: '22:40' },
    { id: 'art-name', title: '命名体系', kind: 'Naming System', version: 'v2', status: 'official', summary: '冷峻、简洁、带旧江湖感；核心名称允许承担隐喻，但控制谐音密度。', updatedAt: '22:43', technical: { artifactRef: 'sha256:8f2…e91', schema: 'NamingSystem@1' } },
    { id: 'art-const', title: '故事宪法', kind: 'Story Constitution', version: 'v3', status: 'official', summary: '名字与记录可以被权力篡改，但真实经历不会被直接改写；主角不得被动接受最终真相。', updatedAt: '22:50', technical: { artifactRef: 'sha256:11a…93b', authority: 'project' } },
    { id: 'art-book', title: '全书战略', kind: 'BookPlan', version: 'v1', status: 'official', summary: '保存全书承诺、宏观人物弧、世界升级、终局边界、主要谜团与各卷意图；不生成整书章纲。', updatedAt: '22:52' },
    { id: 'art-main-contract', title: '全书约束', kind: 'MainContract', version: 'v1', status: 'official', summary: '由故事宪法与全书战略编译出的结构化硬/软约束。', updatedAt: '22:54' },
    { id: 'art-open', title: '开篇三章规则', kind: 'Opening Contract', version: 'v2', status: 'official', summary: '第一章建立异常，第二章建立师门与代价，第三章必须出现身份钩子与主动选择。', updatedAt: '22:57' },
    { id: 'art-ch3', title: '第三章《雨夜来客》', kind: '制作稿', version: 'v3', status: 'candidate', summary: '当前只属于制作区，未进入正式作品。', updatedAt: '23:18' }
  ],
  decisions: [
    { id: 'dec-opening', title: '第三章是否按推荐方向进入修复？', summary: '审核认为雨夜来客已经制造身份悬念，但主角的反应偏被动。推荐增强主角主动试探和一次明确反击。', category: '样章反馈', priority: 'blocking', status: 'pending', createdAt: '23:19', recommendation: '保持剧情不变，增强冲突与主角主动性。', evidence: ['主角连续两段只在回答问题', '玉符异动出现后没有立即行动', '章末钩子成立，不建议改动身份信息'] },
    { id: 'dec-infer', title: '确认 AI 推断的篇幅与平台', summary: '系统推断目标平台为番茄小说，目标篇幅 120–180 万字。', category: 'AI 推断确认', priority: 'normal', status: 'pending', createdAt: '22:35' },
    { id: 'dec-budget', title: '是否保留当前单章预算上限？', summary: '当前单章预算 ¥3.00；前三章平均 ¥2.41。', category: '预算', priority: 'normal', status: 'pending', createdAt: '22:38' }
  ],
  chapters: [
    { id: 'ch1', number: 1, title: '断剑入山', status: 'official', version: 'v2', wordCount: 3268, updatedAt: '23:02', content: [
      '寒江入夜以后，山路上的雪会变得很亮。沈无悔站在最后一级石阶下，抬头看见山门上那两个被风磨得发白的大字。',
      '他背后只带了一只旧包袱，怀里却抱着一柄断剑。剑鞘是新的，剑身只剩七寸，断口像是被什么东西生生咬去。',
      '守门弟子问他从哪里来。他没有回答，只把断剑横在掌心。剑上原本没有纹路，可当山门钟声响起时，铁锈下面慢慢浮出一道墨色细线。',
      '那条线像一个没有写完的名字。沈无悔第一次意识到，这柄剑也许不是兄长留下的遗物，而是一封迟到了很多年的信。'
    ]},
    { id: 'ch2', number: 2, title: '拜师寒江', status: 'official', version: 'v2', wordCount: 3186, updatedAt: '23:10', content: [
      '玄机子没有问沈无悔会不会剑。他只问了三个问题：你为什么来，你怕什么，你愿意拿什么换。',
      '沈无悔说自己来找一个人，怕那个人已经死了，至于愿意拿什么换——他沉默了很久，最后把断剑放在了桌上。',
      '老人没有碰剑。他只是看着断口，像看见一件本不该再次出现的东西。窗外的雪落得很慢，屋里却突然比山道上还冷。',
      '“从今天起，你先学一件事。”玄机子说，“别相信写下来的名字。”'
    ]},
    { id: 'ch3', number: 3, title: '雨夜来客', status: 'producing', version: 'v3', wordCount: 3420, updatedAt: '23:18', content: [
      '雨是在子时以后落下来的。沈无悔被窗纸上的第二声轻响惊醒时，半枚玉符正在枕下发烫。',
      '门外的人没有敲门，只隔着雨幕说：“沈无悔不是你的名字。”',
      '他握住剑，没有出声。那人却像知道他每一个动作，慢慢叫出了另一个名字。一个沈无悔从未听过，却让玉符突然裂开一道新纹的名字。'
    ]}
  ],
  versions: [
    { id: 'ch2-v1', chapterId: 'ch2', label: 'v1', createdAt: '22:59', reason: '首次正式稿前版本', excerpt: '玄机子直接收徒，冲突较弱。', current: false },
    { id: 'ch2-v2', chapterId: 'ch2', label: 'v2 · 当前', createdAt: '23:10', reason: '强化入门问答与名字规则', excerpt: '新增“三个问题”和“不相信写下来的名字”。', current: true }
  ],
  signing: {
    status: 'NOT_STARTED', score: 0, verdict: '前三章尚未全部正式提交，开篇评估未启动。',
    dimensions: [
      { label: '平台匹配', score: 0, note: '等待评估' }, { label: '题材匹配', score: 0, note: '等待评估' },
      { label: '故事承诺清晰度', score: 0, note: '等待评估' }, { label: '第一章钩子', score: 0, note: '等待评估' },
      { label: '三章连续升级', score: 0, note: '等待评估' }, { label: '主角主动性', score: 0, note: '等待评估' },
      { label: '核心机制可持续性', score: 0, note: '等待评估' }, { label: '人物辨识度', score: 0, note: '等待评估' },
      { label: '命名设计质量', score: 0, note: '等待评估' }, { label: '世界规则自洽性', score: 0, note: '等待评估' },
      { label: '同质化风险', score: 0, note: '等待评估' }, { label: '长篇容量', score: 0, note: '等待评估' },
      { label: '预期留存', score: 0, note: '等待评估' }, { label: '商业差异化', score: 0, note: '等待评估' },
      { label: '文学完成度', score: 0, note: '等待评估' }, { label: '内容合规', score: 0, note: '等待评估' }
    ]
  },
  canon: {
    characters: [
      { id: 'shen', name: '沈无悔', role: '主角', status: '正式', location: '寒江剑派', note: '真实姓名未知；持有半枚玉符。' },
      { id: 'xuan', name: '玄机子', role: '上师', status: '正式', location: '寒江剑派', note: '知道断剑来历的一部分，但主动隐瞒。' },
      { id: 'liu', name: '柳青堂', role: '同门', status: '正式', location: '寒江剑派', note: '表面散漫，实际观察力极强。' },
      { id: 'mask', name: '面具先生', role: '神秘来客', status: '候选影响', location: '未知', note: '仅出现在第三章制作稿，尚未进入正式图谱。' }
    ],
    relations: [
      { from: 'shen', to: 'xuan', label: '师徒 · 互有保留', tone: 'neutral' },
      { from: 'shen', to: 'liu', label: '同门 · 初步信任', tone: 'positive' },
      { from: 'xuan', to: 'mask', label: '疑似旧识', tone: 'negative' }
    ],
    timeline: [
      { chapter: '第 1 章', title: '断剑入山', detail: '沈无悔携断剑抵达寒江剑派。', status: 'official' },
      { chapter: '第 2 章', title: '拜师寒江', detail: '玄机子正式收徒，并首次提出“名字不可尽信”。', status: 'official' },
      { chapter: '第 3 章 · 制作影响预览', title: '雨夜来客', detail: '陌生人叫出另一姓名；此事实尚未正式生效。', status: 'candidate' }
    ],
    foreshadows: [
      { id: 'f1', title: '断剑上的墨线', planted: '第 1 章', planned: '第 12–15 章', status: 'open', detail: '可能与被抹去的名字体系有关。' },
      { id: 'f2', title: '半枚玉符', planted: '第 2 章', planned: '第 8 章首次回收', status: 'open', detail: '能够对“被改写的名字”产生反应。' },
      { id: 'f3', title: '玄机子的停顿', planted: '第 2 章', planned: '第一卷后段', status: 'open', detail: '暗示其认识断剑原主人。' }
    ],
    knowledge: [
      { character: '沈无悔', topic: '断剑来历', state: 'unknown', detail: '只知道与失踪兄长有关。' },
      { character: '玄机子', topic: '断剑来历', state: 'conceals', detail: '掌握更多信息，但目前选择隐瞒。' },
      { character: '柳青堂', topic: '玉符异常', state: 'believes', detail: '怀疑沈无悔身上有违禁物，但没有证据。' }
    ]
  },
  settings: {
    provider: 'OpenAI Compatible', model: '创作主模型', endpoint: 'https://api.example.com/v1', monthlyBudget: null, chapterBudget: null,
    privacyMode: 'local-first', sendDraftOnly: true, exportFormat: 'DOCX', platform: '番茄小说', theme: 'light'
  }
}

export interface DemoProjectRecord {
  lifecycle: 'ACTIVE' | 'ARCHIVED'
  createdAt: string
  lastOpenedAt: string
  bundle: ProjectBundle
}

function cloneDemo(overrides: Partial<ProjectBundle['project']>): ProjectBundle {
  const bundle = structuredClone(demoBundle)
  bundle.project = { ...bundle.project, ...overrides }
  return bundle
}

const pausedBundle = cloneDemo({
  id: 'proj-snow-city',
  title: '雪城夜航',
  subtitle: '一座只在午夜出现的北方城市，以及一班无法按时抵达终点的列车',
  platform: '起点中文网',
  genre: '都市异闻 · 悬疑',
  mode: 'AUTO',
  phase: 'STORY_BLUEPRINT',
  runState: 'PAUSED',
  blockReason: 'USER_PAUSE',
  signingStatus: 'NOT_STARTED',
  progress: 22,
  totalCost: 4.18,
  totalElapsed: '11 分钟',
  committedChapters: 0,
  candidateChapters: 0,
  latestReceiptId: 'RCP-20260821-0012',
  revision: 7,
  updatedAt: '2026-08-21 21:42'
})
pausedBundle.decisions = []
pausedBundle.chapters = []
pausedBundle.versions = []
pausedBundle.tasks = pausedBundle.tasks.map((task, index) => ({ ...task, status: index === 0 ? 'active' : 'waiting', progress: index === 0 ? 36 : undefined }))
pausedBundle.timeline = pausedBundle.timeline.map((item, index) => ({ ...item, status: index === 0 ? 'done' : index === 1 ? 'active' : 'locked' }))
pausedBundle.signing = { ...pausedBundle.signing, status: 'NOT_STARTED', score: 0, verdict: '尚未进入开篇生产。' }
pausedBundle.canon = { characters: [], relations: [], timeline: [], foreshadows: [], knowledge: [] }

const completedBundle = cloneDemo({
  id: 'proj-river-archive',
  title: '河灯旧事',
  subtitle: '沿江十二镇的旧案与一盏每年都会回来的河灯',
  platform: '番茄小说',
  genre: '民俗悬疑 · 群像',
  mode: 'DIRECTOR',
  phase: 'COMPLETED',
  runState: 'COMPLETED',
  blockReason: null,
  signingStatus: 'PASS',
  progress: 100,
  totalCost: 286.74,
  totalElapsed: '19 小时 42 分',
  committedChapters: 86,
  candidateChapters: 0,
  latestReceiptId: 'RCP-20260818-8601',
  revision: 441,
  updatedAt: '2026-08-18 18:22'
})
completedBundle.decisions = []
completedBundle.chapters = completedBundle.chapters.map((chapter, index) => ({ ...chapter, status: 'official' as const, number: index + 84, title: index === 0 ? '河灯归岸' : index === 1 ? '旧事沉江' : '尾声', version: 'v1' }))
completedBundle.project.committedChapters = 86
completedBundle.project.candidateChapters = 0
completedBundle.signing = {
  ...completedBundle.signing,
  status: 'PASS',
  score: 91,
  verdict: '开篇评估已通过，作品现已完成。',
  receipt: {
    id: 'SIGN-RCP-RIVER-001', signedAt: '2026/7/2 14:20:00', platformPolicyVersion: 'FQ-2026.07', pitchVersion: 'pitch-a@v2',
    constitutionVersion: 'constitution@v4', namingVersion: 'naming@v3', openingContractVersion: 'opening@v2',
    chapterHashes: ['sha256:river1…', 'sha256:river2…', 'sha256:river3…'], reviewReportId: 'SIGN-REPORT-RIVER', receiptHash: 'sha256:river-sign…'
  }
}

const archivedBundle = cloneDemo({
  id: 'proj-star-dust',
  title: '星尘旅馆',
  subtitle: '每位住客都在房间里遗失一种记忆',
  platform: '晋江文学城',
  genre: '幻想 · 单元故事',
  mode: 'DIRECTOR',
  phase: 'PITCHING',
  runState: 'IDLE',
  blockReason: null,
  signingStatus: 'NOT_STARTED',
  progress: 9,
  totalCost: 1.26,
  totalElapsed: '4 分钟',
  committedChapters: 0,
  candidateChapters: 0,
  latestReceiptId: 'RCP-20260719-0004',
  revision: 3,
  updatedAt: '2026-07-19 13:05'
})
archivedBundle.decisions = []
archivedBundle.chapters = []
archivedBundle.versions = []
archivedBundle.tasks = []
archivedBundle.canon = { characters: [], relations: [], timeline: [], foreshadows: [], knowledge: [] }

export const demoProjectRecords: DemoProjectRecord[] = [
  { lifecycle: 'ACTIVE', createdAt: '2026-08-20 09:30', lastOpenedAt: '2026-08-23 23:18', bundle: demoBundle },
  { lifecycle: 'ACTIVE', createdAt: '2026-08-21 18:10', lastOpenedAt: '2026-08-21 21:42', bundle: pausedBundle },
  { lifecycle: 'ACTIVE', createdAt: '2026-06-18 10:12', lastOpenedAt: '2026-08-18 18:22', bundle: completedBundle },
  { lifecycle: 'ARCHIVED', createdAt: '2026-07-19 12:40', lastOpenedAt: '2026-07-19 13:05', bundle: archivedBundle }
]

export const starterMarket = structuredClone(demoBundle.market)

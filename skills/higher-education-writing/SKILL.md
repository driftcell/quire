---
name: higher-education-writing
description: Drafts and reviews Chinese higher-education institutional reports (discipline planning, benchmarking, degree-program applications, construction summaries, and official university documents). Use when authoring or editing formal higher-ed reports or running editorial QA.
---

# 高等教育研报写作与 9 步集群工作流

服务对象为高校校领导、发展规划处、学科建设办及评审专家。核心目标是交付**高信息密度、证据可追溯、可直接用于决策汇报**的正式材料。不要先写正文再找数据装饰，而要先锁定指标、形成图表证据，再写结构归因。

## 按需加载参考文件（一级直连，按任务取用）

| 文件 | 适用场景 | 谁来读 |
| :--- | :--- | :--- |
| `references/examples.md` | 首次起草正文、改写标题或撰写四层结语时对齐文风 | `main` / `writer` |
| `references/showcase-and-cases.md` | 撰写或审校“成效展示、亮点总结、典型案例”章节 | `writer` / `reviewer` |
| `references/gov-documents.md` | 撰写或审校正式政府/校级红头公文 | `writer` / `reviewer` |
| `references/review-rubric.md` | 成稿后的独立事实核对、结构审查与套话清洗 | **仅 `reviewer`** |

## 报告类型 ↔ 指标与信源映射矩阵

先判定报告类型，再决定必查指标组合。内部数据源按 `panorama_report_*`（全景指标）、`shuangyou_*`（双优建设）、`subjectcloud_*`（学科评估）、`subjectcloud_talent_*`（人才数据）取用；外部权威信源只使用 `.gov.cn` / `.edu.cn`、基金委正式公告、ESI 公报或学校正式文件，不使用自媒体补齐核心结论。

| 报告类型 | 必查核心指标组合 | 首选信源 |
| :--- | :--- | :--- |
| 冲刺“双一流”报告 | 学科评估档位、国自然/国社科立项与经费、重大重点项目、ESI 学科、国家级平台与奖励、人才队伍 | `subjectcloud_*`、`panorama_report_*`、`shuangyou_*`；教育部/省政府正式文件、基金委、ESI 公报 |
| 省属“双优”/高水平大学报告 | 双优建设任务完成度、预算执行与专项资金、优势特色学科、平台团队、产教融合成效 | `shuangyou_*`、`panorama_report_*`；省级双优名单、财政专项文件、学校正式材料 |
| 博硕士学位点申报论证 | 学科评估与学位点梯队、国自然/国社科、导师与团队、平台支撑、代表性成果、人才引育 | `subjectcloud_*`、`subjectcloud_talent_*`、`panorama_report_*`；学位委员会正式文件、基金委、ESI 公报 |
| 年度科研竞争力对标 | 立项总量与结构、经费与增长率、重大重点项目、平台成果、人才与团队、学科排名变化 | `panorama_report_*`、`subjectcloud_*`、`subjectcloud_talent_*`；基金委、ESI 公报、学校科研年报 |
| 成效案例/红头公文 | 建设任务、量化成效、典型项目、机制创新、可复制经验 | `panorama_report_*`、`shuangyou_*`；学校/政府正式文件、官方媒体、项目验收材料 |

## 9 步弹性写作与批注审校工作流

### Step 1 资料收集与类型-指标映射

- 先按上表确定报告类型与必查指标，再决定章节是否成立；没有数据支撑的章节不硬设。
- 明确对标高校组、时间窗口与统计口径。若这些条件无法从上下文推断，先用 `ask_user` 一次确认。
- 统一 `upsert_sources` 登记来源与 `source_id`，确保全集群共享同一套引用。
- 数据探查结论先于章节骨架。把零记录、缺失项和口径边界整理成主编的 `shared_facts` 只读事实锁，后续 `writer` 与 `reviewer` 不得自行换算。

### Step 2 弹性框架与占位标题

- 依据数据探查结果拟定章节骨架：有数据支撑才设章，发现结构不合理时用 `apply_operations` 调整，不为了凑九章而硬写。
- **标题后置**：建桩期的一级/二级章节可以先放职责占位标题，例如 `## [待拟标题：国自然立项体量与结构对标]`，避免在内容和图表未落地前硬编空洞标题。
- `[待拟标题：...]` 只是建桩占位符，定稿前必须替换为正式结论型/提炼式标题。`QR401` 负责拦截未完成占位符；`QR418` 对冒号后已有具体对象的标题不做普通计数标题误报。

### Step 3 数据与视觉编排先行

- `analyst` 先把定量对比落为三级表头学术三线表（`Table`），把趋势与结构分化落为原生 ECharts 6 图，把机制架构、建设路线和核心判断卡落为 `CustomHtmlBlock` 或 `Quote`。
- 每张图执行 `preview_echarts` 视觉自检后再 `commit_chart`；`note` 写明统计口径、时间截点、量纲单位和排除范围，`source_ids` 绑定来源，`fallback_summary` 写出极值、均值或关键差值。
- 图表是正文的主要证据载体，避免正文成为明细数字的流水账。

### Step 4 图文结合正文写作

- `writer` 动笔前必须 `read_blocks` 读取本章已落库图表的 `title`、`note`、`fallback_summary` 与表格列数据。
- **图表已展示的明细数字绝不在正文逐一罗列报幕**。正文只写：首句带量纲核心判断 → 纵向拐点与横向位次反差 → 结构拆解与深层机制归因。
- 每段首句包含时间窗口、核心指标、绝对值/增幅和对标位次；同一组基础数据只在唯一主讲章完整展开，其他章节简短回指。
- 本章内容和图表全部到位后，再把 `[待拟标题：...]` 初拟为符合高教规范的结论型/提炼式标题。

### Step 5 独立审校与锚定批注（`reviewer` Round 1）

- `reviewer` 检查跨章重复表述、上下口径不一（对照 `shared_facts`）、文表数字失配、残留黑话套话和 A4 版式。
- 发现问题必须调用 `add_comment(doc_id, block_id, quote, body)` 在对应块上创建正式批注，标注 `[Blocker]` / `[Major]` / `[Minor]`，并给出可直接替换的修订建议，不能只在文字回复里罗列问题。
- 返回 `subagent_run_id` 与状态：`CHANGES_REQUESTED` 或 `APPROVED`，供主编续接同一个 `reviewer`。

### Step 6 主编全局视角改稿与批注回复

- `main` 调用 `list_comments(doc_id, status="open")`，站在全篇视角统筹修改，消除各子代理因局部视野导致的跨章重复表述与口径冲突。
- 每修好一处调用 `reply_comment(..., mark_addressed=true, addressed_rev=最新版本号)`，只修改批注锚定范围，不顺手重写全篇。

### Step 7 & 8 Continue 同一个 `reviewer` 复核循环 + 全篇标题终定

- `main` 调用 `spawn_subagents` 并传入 `resume_run_id: reviewer_run_id`，让**同一个 `reviewer`** 检查所有 `addressed` 批注及对应最新块是否彻底修复。
- 若仍不通过则继续追评批注，直到 `reviewer` 返回 `APPROVED`，上限 2~3 轮；续接时保留前序对话、批注和工具结果，避免每次从零开始审校。
- 审校完全通过后，`main` 统揽全篇内容与图表，最终打磨并敲定全篇各级标题：不得残留任何 `[待拟标题]`，各章标题句式协调且信息饱满。

### Step 9 按需样式与定稿交付

- **克制调用 `designer`**：用户没有特殊样式、封面或版式要求时，直接使用 `create_report` 默认的庄重学术样式，确认 `lint_document` 零 error 后交付，无需调用样式 skill 或派发 `designer`。
- 仅当用户明确提出样式/装帧定制需求时才调用 `designer` 配置封面、目录、页眉页脚与主题 CSS token。

## 撰稿核心契约（正向规则）

1. **明确写作名义与称谓**：
   - **第三方分析报告**（对标、研判、调研）：称委托校全称或“学校”；客观陈述优劣，不替学校表态。
   - **以学校名义撰写**（申报书、建设方案、总结）：称“我校”；目标与举措严格限于已公开或校方提供材料。
   - 固定提法：“双一流”“双优”“十五五”加引号；ESI 阈值写为“ESI全球前1%”“前1‰”“前万分之一”。
2. **先定判断清单，再分章写正文**：
   - 每章围绕 1 个核心判断展开，前章结论是后章推演前提；同一组基础数据只在唯一主讲章完整展开。
   - 段落按分论点组织：**首句亮带量纲判断（时间窗口 + 指标 + 绝对值/增幅 + 对标位次）→ 纵向趋势与横向对标 → 结构拆解与机制归因**。
3. **口径进表注，正文只写结论**：
   - 统计时间截点、包含/剔除范围集中写入图表 `note`；缺数据在表内标 `—` 并在对话汇报中告知用户，不写进报告正文。
   - 直接用数量、占比、位次与增幅论证，不写“研究设计”“样本”“Spearman相关系数”等学术论文腔。
4. **四层结语收束**：置于建议之后、附录之前——① 回扣开篇背景与核心问题 → ② 综合研判 → ③ 推出建设方向 → ④ 落到学校/学科担当（不首次引入新数字）。

## 路由补充

- **单 Agent 快轨（1~3 个章节的专项简报、单表/单图分析或局部修订）**：`get_protocol_guide("quickstart")` -> `create_report` -> `upsert_sources` -> `preview_echarts` 审图后 `commit_chart` -> `apply_operations` -> `lint_document` 交付，不启动集群。
- **多 Agent 集群（4 个及以上章节的综合调研报告、多校多维度对标或学科建设规划书）**：严格按上述 9 步执行，先图后文、独立审校、批注闭环、Continue 复核，最后按需装帧。

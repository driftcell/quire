---
name: higher-ed-report-cluster
description: 高等教育与科研发展研报（高校学科规划、国自然/社科基金对标、学位点建设、双一流/双优冲刺、人才引育评估）的单Agent与多Agent集群编排规范及正向学术公文写作范式。
---

# 高等教育研报编排与正向写作范式

## 一、核心写作哲学：以“高信息密度正向句法”替代“负面禁用词”

高校机关（发展规划处、学科建设办、科研院）与专业高教咨询报告的核心竞争力是**可核验的增量信息密度**。不要在脑中检索“不能用什么词”，而是严格用以下**四条正向句法契约**填满每一个自然段：

1. **结论先行，首句即判断（Topic Sentence with Metric）**：
   - 每个章节或核心段落的首句直接给出带量纲的判断（包含：**时间窗口 + 核心指标 + 绝对值/变化率 + 省内/同梯队排位**）。
   - 示范：`2021—2025年，浙江工业大学国家自然科学基金立项总数由158项增至214项，年均复合增长7.9%，在浙江省属高校中连续五年稳居首位，但面上项目占比仍低于同梯队江苏高校均值8.4个百分点。`
2. **三维对标法（纵向看增速、横向看位次、结构看短板）**：
   - 任何单一数据本身没有意义。呈现指标时，必须绑定至少一个参照系：**本校历史基期（纵向）**、**标杆/同城/同类型高校（横向）**或**结构占比（青年/面上/重点、一级学科分布）**。
3. **精准使用“转折辨析”与“口径边界说明”**：
   - **允许且鼓励有实质信息增量的对比转折**：当破除表面数据的误区时（如“总量增长但人均产出下降”、“立项数领先但重大重点项目缺位”），直接陈述两个维度的结构反差。
   - **用“统计口径界定”替代“空泛免责套话”**：严禁写“由于水平有限，仅供参考”等无意义免责；但**必须保留严谨的统计口径边界**（如：`注：本节国自然统计口径包含面上、青年及地区基金，不含年底增补的联合基金与专项项目`），将口径写在表格/图表的 `note` 字段或正文括号内。
4. **对策建议必须绑定“主体 + 靶向指标 + 可落地机制”**：
   - 提出高校建设建议时，直接指向具体抓手（如：博士点培育学科的带头人引育、省部级重点实验室重组、交叉学科经费池配置），避免空喊口号。

---

## 二、路由判定：何时走单 Agent 快轨 vs 何时启动集群

- **单 Agent 快轨（`main` 直接执行）**：
  - 用户仅要求生成 1~3 个章节的专项简报、单表/单图分析，或对现有报告进行批注修订。
  - 执行顺序：`get_protocol_guide("quickstart")` -> `create_report` -> `upsert_sources` -> `preview_echarts` & `commit_chart` -> `apply_operations`（三线表与正文） -> `set_cover` & `configure_page_chrome` -> `lint_document` -> 交付。
- **多 Agent 集群模式（`main` 调度 `analyst` / `writer` / `designer` / `reviewer`）**：
  - 用户要求生成 4 个及以上章节的综合调研报告、多校多维度对标报告或学科建设规划书。

---

## 三、集群五阶段编排协议（Cluster Orchestration DAG）

### Phase 1：立项、口径确认与大纲锚桩（`main`）
1. 若核心对标范围（如对标哪几所高校、统计年份区间）不明确且无法从上下文推断，调用 `ask_user` 一次性确认。
2. 调用 `create_report` 创建文档，调用 `upsert_sources` 录入所有基础数据源，确保全集群共享同一套 `source_id`。
3. 调用 `apply_operations` 写入全篇一级标题（`sec1_h1`, `sec2_h1`, `sec3_h1`...），在文档中建立稳定的物理锚点。

### Phase 2：数据底表与可视化先行（委派 `analyst`）
- 通过 `spawn_subagents` 启动 `analyst`，明确传入：
  - 目标章节锚点（如 `after_block: sec1_h1`）、分配的块前缀（如 `tbl_sec1_`, `fig_sec1_`）；
  - 完整的数据明细与已登记的 `source_id`；
  - 要求 `analyst` 完成多级表头三线表落库，并对每张 ECharts 图表执行 `preview_echarts` 视觉自检后再 `commit_chart`。

### Phase 3：分章节无冲突并行撰稿（并行委派多个 `writer`）
- 待 `analyst` 返回已落库的表/图 `block_id` 及核心数据极值后，通过 `spawn_subagents` 并行启动多个 `writer`，**按章节物理隔离 `block_id` 前缀**：
  - `writer` A：负责第 1 章（前缀 `sec1_p*`），锚定在 `sec1_h1` 及 `tbl_sec1_*` 周围展开数据分析；
  - `writer` B：负责第 2 章（前缀 `sec2_p*`），锚定在 `sec2_h1` 及 `fig_sec2_*` 周围展开深度剖析；
  - `writer` C：负责第 3 章（前缀 `sec3_p*`），撰写靶向建设路径与指标拆解。
- 每个 `writer` 的任务描述中必须包含：分配的 `block_id` 前缀、可用的 `source_id` 清单、本章已落库的图表 `block_id` 及其核心数值结论。

### Phase 4：出版装帧与独立红线审校（并行委派 `designer` + `reviewer`）
- `designer`：调用 `set_cover`（高校报告默认推荐 `classic` 或 `official` 风格，填写 `kicker` 眉题、`authors` 与 `date`）、`configure_toc`、`configure_page_chrome`（启用三栏学术公文页眉页脚）、`configure_references`。
- `reviewer`：调用 `read_skill_resource("higher-ed-report-cluster", "resources/editorial-redlines.md")`，执行 `lint_document` 与 `preview_report`，核查：
  1. 表内数字、图内数字与正文引述数字是否 100% 一致；
  2. 是否存在无数据支撑的空洞套话；
  3. A4 分页视图下是否有表格/图表溢出或标题孤行。

### Phase 5：主编合龙与交付（`main`）
- 汇总 `reviewer` 返回的具体 `block_id` 问题清单，调用 `apply_operations` 一次性定点修补，再次确认 `lint_document` 零 error 后向用户汇报。

---
name: higher-education-research
description: Orchestrates parallel multi-track higher-education deep research across internal data warehouses (panorama_report, subjectcloud, shuangyou) and authoritative external web sources (.gov.cn, .edu.cn, NSFC, MOE communiqués), enforcing domain-scoped source registration and verbatim evidence excerpts.
---

# 高等教育多维深度调研与并行取证规范（Parallel Research Swarm Skill）

本技能专为 Quire **Step 1 并行深度调研阶段**设计，供首席主编（`main`）拆解调研赛道，并由并行派发的 **2—4 位深度调研专员（`researcher` 子代理集群）** 加载执行。

核心理念：**严禁由主代理单线程浅尝辄止地查几个表层总数就草率建章立论**。复杂的高校发展规划、学科竞争力对标、博硕士点申报与“双一流/双优”研报，必须通过并行 `researcher` 子代理集群对**四条正交调研赛道**展开下钻式深潜（Drill-Down），将“总量指标 → 分年趋势 → 院系/学科结构分布 → 标志性成果明细 → 官方政策原文摘录”全部取齐并分域登记为可溯源的 `SourceEntry`。

## 按需加载参考文件

| 文件 | 适用场景 | 谁来读 |
| :--- | :--- | :--- |
| `references/track-playbooks.md` | 四大调研赛道的工具下钻调用链（`panorama_report_*` / `subjectcloud_*` / `shuangyou_*` / `deepseek_fetch`）、高频参数陷阱与分域信源 ID 命名规范 | `researcher` / `main` |

---

## 一、并行调研集群的四维正交赛道（4 Orthogonal Research Tracks）

在启动 4+ 章综合研报时，`main` 先完成高校实体锚定（确认目标校与对标校名单、统计时间窗，并先行 `create_report` 生成 `doc_id`），随即调用 `spawn_subagents` 并行派发 **2—4 位 `researcher` 子代理**，按以下正交赛道分工深潜：

| 赛道代号 | 调研专员分工（`display_name`） | 核心取证范围与必下钻维度 | 首选工具链与信源 ID 前缀 |
| :--- | :--- | :--- | :--- |
| **赛道 A** | `学科与学位点架构调研专员` | 第四/五轮学科评估档位、年度学科发展指数得分与百分位、博士/硕士一级学科授权点与专业学位点结构、ESI 全球前 1% 与前 1‰ 学科进位及千分位差距 | `subjectcloud_overview`（单次 ≤10 个学科）、`subjectcloud_detail`、`subjectcloud_result`、`panorama_report_query`；信源前缀 `sc-discipline-*`、`pr-degree-*` |
| **赛道 B** | `师资规模与高层次人才调研专员` | 专任教师总量、生师比、博士学位教师占比、两院院士/长江/杰青/万人计划等国家级领军人才与国家级青年人才存量及近五年增量、高层次人才在各主干学科的集聚度与断层 | `subjectcloud_talent_search`、`subjectcloud_talent_batch`、`subjectcloud_talent_detail`、`panorama_report_query`；信源前缀 `sc-talent-*`、`pr-faculty-*` |
| **赛道 C** | `纵向科研、平台基地与奖励调研专员` | 国家自然科学基金（面上/青年/重点/重大/杰青优青/联合基金）与国家社科基金历年立项数及经费、到账总经费结构、全国重点实验室/国家工程研究中心/省部级重点实验室矩阵、国家三大奖与教育部科技奖标志性成果 | `panorama_report_query`、`panorama_report_drill`（下钻项目/平台/奖项明细）；信源前缀 `pr-research-*`、`pr-platform-*`、`pr-award-*` |
| **赛道 D** | `双优产教、区域战略与官方政策原文调研专员` | 双优计划建设任务与产业契合度、现代产业学院与专利转化、教育部/省政府最新高教统计公报与“双一流/双优”政策文件原文、目标校与对标校官网发布的《“十四五”规划》《本科教学质量报告》《部门决算》原文 | `shuangyou_profile`、`shuangyou_industry_source`、`shuangyou_intel_*`、`shuangyou_planning_*` + `deepseek_search` + **`deepseek_fetch`（必读原文）**；信源前缀 `sy-industry-*`、`gov-policy-*`、`edu-official-*` |

> **弹性编排原则**：
> - **4–7 章大型跨校对标综合研报**：并行派发 **3–4 位 `researcher`** 全面覆盖赛道 A/B/C/D。
> - **专项论证报告（如单一学科申博论证或科研专项对标）**：并行派发 **2 位 `researcher`**（例如 1 位专攻内部库全量学科+人才+科研下钻，1 位专攻教育部学位点基本条件文件、省内政策与对标校官网公开原文抓取）。

---

## 二、铁律级取证与信源入库规范（Zero-Fabrication Evidence Protocol）

### 1. 外部网页必须 `deepseek_fetch` 读取原文后方可入库
- 网页搜索（`deepseek_search`）返回的 snippet 只是残缺预览，**严禁仅凭搜索摘要脑补数字或直接把 snippet 当作已核实摘录写入 `upsert_sources`**。
- 凡准备采信并注册为 `SourceEntry` 的外部网页，**必须调用 `deepseek_fetch(url)` 读取网页正文**，并将包含核心统计数字、时间窗、政策条款或机构定性的**原句逐字复制到 `SourceEntry.excerpt`** 中。
- **权威信源白名单**：只采信政府主管部门（`.gov.cn`，如教育部、科技部、省教育厅、省政府）、高校官方网站与信息公开网（`.edu.cn`）、国家自然科学基金委/全国哲社办正式公告、Clarivate ESI 官方公报或权威学术统计年鉴。**严禁采信百家号、搜狐号、网易号、头条号、知乎、百度百科、个人博客或第三方自媒体汇总帖**。

### 2. 内部数据库分域注册与“反口袋信源”铁律
- 严禁把所有内部数据库查出来的学科、人才、科研、平台数据全部塞进一个名为“高校公开数据大汇编”或“青塔数据库综合记录”的单一口袋信源！
- 每个 `researcher` 必须按自身负责的**业务数据域**分别调用 `upsert_sources` 注册权责清晰、颗粒度适中的独立信源（如 `sc-discipline-eval`、`sc-talent-leading`、`pr-nsfc-2021-2025`、`pr-platform-national`、`sy-industry-match`），并在每条信源的 `excerpt` 中完整列出该数据域核实到的**各校分年/分项精确原值与统计口径**，确保后续 `analyst`、`writer`、`humanizer` 与 `reviewer` 随时调用 `list_sources` 都能在 `excerpt` 中逐字核对每一个引用数字！
- **并发入库零冲突**：`upsert_sources` 在后端持有文档级行锁并支持 URL/标题自动去重合并，多位并行 `researcher` 可直接并发调用 `upsert_sources` 将各赛道信源实时写入报告。

### 3. `subjectcloud` 双评价体系严格隔离（`moe` vs `index`）
- `subjectcloud` 中同时包含两套截然不同的学科数据口径：
  1. **`moe`（教育部学科评估公开/折算档位口径）**：对应第四轮/第五轮学科评估的 A+/A/A-/B+/B/B-/C+/C/C- 档位；
  2. **`index`（年度学科发展指数口径）**：基于年度公开纵向指标计算的连续得分、全国位次与百分位档位。
- `researcher` 在取数和写入 `SourceEntry.excerpt` 时，**必须显式标明每组数字属于 `教育部学科评估口径` 还是 `年度学科发展指数口径`**，绝不能将两套体系的档位或排名混为一谈！

### 4. 下钻到底（Drill-Down Depth），拒绝只报表面总数
- 发现某高校某项总量指标突出或滞后时（如“国自然立项总数高，但重大重点少”或“省级平台多，但国家级平台为零”），`researcher` 必须主动调用 `panorama_report_drill`、`subjectcloud_detail` 或 `subjectcloud_talent_detail` 下钻一层，查清：
  - **结构分布**：具体集中在哪些优势学科或头部学院？哪些主干学科处于空白？
  - **跨期拐点**：2021—2025 年哪一年出现了跃升或滑坡？
  - **标志性实体**：具体的国家级平台名称、重大项目或领军学者所属方向是什么？
- 只有把这些下钻明细写进 `SourceEntry.excerpt` 和调研卷宗，后续 `analyst` 才能画出有信息量的结构分化图表，`writer` 才能写出“判断 → 结构拆解 → 机制归因 → 制度抓手”的四层深度论证！

---

## 三、`researcher` 交付给主编的标准《调研证据卷宗》（Evidence Dossier）

每位 `researcher` 完成本赛道全部取数与 `upsert_sources` 登记后，必须向 `main` 返回结构化的四部分调研证据卷宗：
1. **已入库信源清单（Registered Sources）**：逐条列出已在 `upsert_sources` 登记成功的 `canonical_id`、信源标题、数据覆盖范围与统计口径说明。
2. **核心量化事实矩阵（Verified Quantitative Matrix）**：按“指标名称（单位/时间窗/口径） | 目标校数值 | 各对标校数值 | 核心位次与差值”清晰列出本赛道全部已核实数据。
3. **下钻结构发现与关键实体（Drill-Down Structural Insights）**：提炼 2—4 条仅凭表面总数看不出来的深层结构反差（如分院系集中度、青年与领军断层、平台与学位点错位、最新政策文件硬门槛），附带具体平台/学科/政策文件全称。
4. **跨源口径差异与未收录项声明（Discrepancies & Missing Data）**：如实报告不同信源间的数值差异（并列原值与各自口径，不私自抹平）以及确无记录的缺失指标（标注 `暂未收录`），供主编统一裁定 `shared_facts`。

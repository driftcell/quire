import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentAvatar } from "@/components/chat/agentAvatar";
import { buildChatRichContext } from "@/components/chat/chatRichContext";
import { HistoricalRunTrace } from "@/components/chat/ExecutionTraceSection";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import {
  buildHistoricalMilestones,
  buildLiveMilestones,
  buildSemanticActionPills,
} from "@/components/chat/taskMilestones";
import {
  buildHistoricalRunStatusModel,
  buildLiveRunStatusModel,
  type RunStatusModel,
} from "@/components/chat/traceSegments";
import type { RunTraceSummary } from "@/contracts/session-protocol";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

describe("RunStatusModel & Borderless Editorial Stream", () => {
  test("live ledger focuses on current work, next steps, and specialist status", () => {
    const liveMain: LiveAgentTrace = {
      runId: "run_1",
      agentId: "main",
      isStreaming: true,
      startedAtMs: 1_000,
      taskFocus: {
        title: "正在统筹撰写双优建设报告",
        detail: "核对高校数据",
        todos: [
          { title: "第一阶段：构建报告框架", status: "completed" },
          {
            title: "正在统筹撰写双优建设报告",
            status: "in_progress",
            detail: "核对高校数据",
          },
          { title: "第三阶段：全局质检与终审", status: "pending" },
        ],
      },
      segments: [
        {
          kind: "thinking",
          text: "内部推理过程不应在用户聊天界面中暴露",
        },
        {
          kind: "execution",
          tools: [
            {
              toolCallId: "tc_title",
              toolName: "set_session_title",
              status: "completed",
              inputPreview: JSON.stringify({ title: "双优建设报告" }),
            },
            {
              toolCallId: "tc_desc_1",
              toolName: "describe_task",
              status: "completed",
              inputPreview: JSON.stringify({
                title: "第一阶段：构建报告框架",
                detail: "创建封面与章节骨架",
                section_id: "s1-intro",
              }),
            },
            {
              toolCallId: "tc_0",
              toolName: "create_report",
              status: "completed",
              inputPreview: JSON.stringify({ title: "双优建设报告" }),
            },
            {
              toolCallId: "tc_desc_2",
              toolName: "describe_task",
              status: "completed",
              inputPreview: JSON.stringify({
                title: "正在统筹撰写双优建设报告",
                detail: "核对高校数据",
                section_id: "s2-metrics",
              }),
            },
            {
              toolCallId: "tc_1",
              toolName: "apply_operations",
              status: "running",
              inputPreview: JSON.stringify({ section_id: "s2-metrics" }),
            },
          ],
          subagentRunIds: ["sub_1"],
        },
      ],
    };

    const subagents: LiveSubagentRun[] = [
      {
        subagentRunId: "sub_1",
        agentName: "researcher",
        displayName: "数据检索员",
        task: "检索浙江海洋大学近三年科研指标",
        status: "running",
        trace: {
          runId: "sub_1",
          agentId: "researcher",
          isStreaming: true,
          taskFocus: {
            title: "检索国自然立项数据",
            todos: [
              { title: "查询教育部经费公报", status: "completed" },
              { title: "检索国自然立项数据", status: "in_progress" },
            ],
          },
          segments: [
            {
              kind: "text",
              text: "已核验国自然立项数据，形成可直接用于报告的分析结论。",
            },
          ],
        },
      },
    ];

    const milestones = buildLiveMilestones(liveMain, subagents);
    expect(milestones).toHaveLength(2);
    expect(milestones[0]?.title).toBe("第一阶段：构建报告框架");
    expect(milestones[0]?.status).toBe("completed");
    expect(milestones[1]?.title).toBe("正在统筹撰写双优建设报告");
    expect(milestones[1]?.status).toBe("running");

    const model = buildLiveRunStatusModel(liveMain, subagents, 14);
    expect(model.status).toBe("running");
    expect(model.title).toBe("正在统筹撰写双优建设报告");
    // set_session_title is hidden from visible tools
    expect(model.toolCount).toBe(2);
    expect(model.completedToolCount).toBe(1);
    expect(model.todos).toHaveLength(3);
    expect(model.subagents).toHaveLength(1);
    expect(model.subagents[0]?.activityText).toBe("检索国自然立项数据");
    expect(model.subagents[0]?.todos).toHaveLength(2);
    expect(model.elapsedLabel).toBe("14秒");

    const html = renderToStaticMarkup(
      <RunActivityHeader
        model={model}
        isStreaming={true}
        detailsOpen={false}
        onToggleDetails={() => {}}
      />,
    );
    expect(html).toContain('data-live-header="true"');
    expect(html).toContain('data-slot="agent-status"');
    expect(html).toContain('data-slot="agent-plan"');
    expect(html).toContain('data-slot="subagent-list"');
    expect(html).toContain('data-presentation="ledger"');
    expect(html).not.toContain('data-slot="subagent-todo-list"');
    expect(html).not.toContain('data-slot="agent-plan-progress"');
    expect(html).not.toContain('data-slot="subagent-progress"');
    expect(html).not.toContain('data-live-progress-tools="true"');
    expect(html).toContain("第一阶段：构建报告框架");
    expect(html).toContain("正在统筹撰写双优建设报告");
    expect(html).toContain("第三阶段：全局质检与终审");
    expect(html).toContain("执行计划，共 3 步，当前第 2 步");
    expect(html).toContain('data-presentation="todo-window"');
    expect(html).toContain('data-presentation="avatar-stack"');
    expect(html.split('data-slot="main-todo-item"').length - 1).toBe(3);
    expect(html).toContain('data-focus="true"');
    expect(html).toContain("正在统筹撰写双优建设报告");
    expect(html).not.toContain("主代理");
    expect(html).toContain('data-subagent-pill="sub_1"');
    expect(html).toContain("展开并定位");
    expect(html).toContain("ring-[#1D4ED8]");
    expect(html).not.toContain("grayscale");
    expect(html).toContain("检索国自然立项数据");
    // Never sticky (prevents occluding QuestionCard on scroll)
    expect(html).not.toContain("sticky");
    // Zero spinning loader icons or shimmer text in the header, plan, or subagent cards
    expect(html).not.toContain("animate-spin");
    expect(html).not.toContain("shimmer");
    // Elapsed time must appear only ONCE in the header
    expect(html.split("14秒").length - 1).toBe(1);

    const expandedHtml = renderToStaticMarkup(
      <RunActivityHeader
        model={model}
        isStreaming={true}
        detailsOpen={true}
        onToggleDetails={() => {}}
      />,
    );
    expect(expandedHtml).toContain("第一阶段：构建报告框架");
    expect(expandedHtml).toContain('data-presentation="full-todos"');
    expect(expandedHtml).toContain('data-subagent-id="sub_1"');
    expect(expandedHtml).toContain('tabindex="-1"');
    expect(expandedHtml).toContain('data-slot="subagent-todo-list"');
    expect(expandedHtml).toContain("查询教育部经费公报");
    expect(expandedHtml).toContain("检索国自然立项数据");
    expect(expandedHtml).not.toContain('data-slot="subagent-output"');
    expect(expandedHtml).not.toContain('data-slot="semantic-action-pills"');
    expect(expandedHtml).not.toContain(
      "已核验国自然立项数据，形成可直接用于报告的分析结论。",
    );
    // Raw chain-of-thought thinking must never leak into the user-facing UI
    expect(expandedHtml).not.toContain("内部推理过程不应在用户聊天界面中暴露");
    expect(expandedHtml).not.toContain("animate-spin");
  });

  test("collapsed todo windows handle boundaries and subagent resume coloring", () => {
    const makeModel = (
      statuses: Array<RunStatusModel["todos"][number]["status"]>,
      subagentStatus: RunStatusModel["subagents"][number]["status"] = "running",
    ): RunStatusModel => ({
      title: "当前任务",
      status: "running",
      toolCount: 0,
      completedToolCount: 0,
      recentMainTools: [],
      subagents: [
        {
          id: "sub_1",
          actor: "researcher",
          status: subagentStatus,
          activityText: "核对高校数据",
          toolCount: 0,
          completedToolCount: 0,
          recentTools: [],
          todos: [],
        },
      ],
      milestones: [],
      todos: statuses.map((status, index) => ({
        id: `todo-${index + 1}`,
        title: `任务 ${index + 1}`,
        status,
      })),
    });
    const renderCollapsed = (model: RunStatusModel) =>
      renderToStaticMarkup(
        <RunActivityHeader
          model={model}
          isStreaming={true}
          detailsOpen={false}
          onToggleDetails={() => {}}
        />,
      );

    const first = renderCollapsed(
      makeModel(["in_progress", "pending", "pending"]),
    );
    expect(first.split('data-slot="main-todo-item"').length - 1).toBe(3);
    expect(first.indexOf('data-focus="true"')).toBeLessThan(
      first.indexOf("任务 2"),
    );

    const last = renderCollapsed(
      makeModel(["completed", "completed", "in_progress"]),
    );
    expect(last.split('data-slot="main-todo-item"').length - 1).toBe(3);
    expect(last.indexOf('data-focus="true"')).toBeGreaterThan(
      last.indexOf("任务 2"),
    );

    const single = renderCollapsed(makeModel(["in_progress"]));
    expect(single.split('data-slot="main-todo-item"').length - 1).toBe(1);

    const completed = renderCollapsed(makeModel(["in_progress"], "completed"));
    expect(completed).toContain("grayscale");
    const resumed = renderCollapsed(makeModel(["in_progress"], "running"));
    expect(resumed).not.toContain("grayscale");

    const crowded = makeModel(["in_progress"]);
    crowded.subagents = Array.from({ length: 9 }, (_, index) => ({
      id: `sub-${index + 1}`,
      actor: `researcher-${index + 1}`,
      status: "completed",
      activityText: "核对高校数据",
      toolCount: 0,
      completedToolCount: 0,
      recentTools: [],
      todos: [],
    }));
    const stack = renderCollapsed(crowded);
    expect(stack.split("data-subagent-pill=").length - 1).toBe(6);
    expect(stack).toContain("+3");

    const activeCrowd = makeModel(["in_progress"]);
    activeCrowd.subagents = Array.from({ length: 9 }, (_, index) => ({
      id: `sub-${index + 1}`,
      actor: `researcher-${index + 1}`,
      status: index < 8 ? "running" : "completed",
      activityText: "核对高校数据",
      toolCount: 0,
      completedToolCount: 0,
      recentTools: [],
      todos: [],
    }));
    const activeStack = renderCollapsed(activeCrowd);
    expect(activeStack.split("data-subagent-pill=").length - 1).toBe(8);
    for (let index = 1; index <= 8; index += 1) {
      expect(activeStack).toContain(`data-subagent-pill="sub-${index}"`);
    }
    expect(activeStack).not.toContain('data-subagent-pill="sub-9"');
    expect(activeStack).toContain("+1");
    expect(activeStack).toContain("flex-wrap");
  });

  test("buildHistoricalRunStatusModel preserves structure after completion and supports chat-only upsert_sources", () => {
    const trace: RunTraceSummary = {
      run_id: "run_1",
      status: "completed",
      duration_ms: 14_200,
      task_focus: { title: "已完成双优建设报告", section_id: "sec_1" },
      segments: [
        {
          kind: "thinking",
          text: "历史内部思考文本同样不向终端用户展示",
        },
        {
          kind: "execution",
          tools: [
            {
              tool_call_id: "tc_d1",
              tool_name: "describe_task",
              status: "completed",
              input_preview: JSON.stringify({
                title: "阶段一：采集经费来源",
                detail: "汇总教育部与省级拨款",
                todos: [
                  { title: "阶段一：采集经费来源", status: "in_progress" },
                  { title: "已完成双优建设报告", status: "pending" },
                ],
              }),
            },
            {
              tool_call_id: "tc_s1",
              tool_name: "upsert_sources",
              status: "completed",
              input_preview: JSON.stringify({
                sources: [
                  {
                    id: "moe-2024",
                    title: "2024年全国教育事业发展统计公报",
                    publisher: "中华人民共和国教育部",
                    url: "https://www.moe.gov.cn/jyb_sjzl/",
                    evidence: "全国共有高等学校 3119 所。",
                  },
                ],
              }),
            },
            {
              tool_call_id: "tc_d2",
              tool_name: "describe_task",
              status: "completed",
              input_preview: JSON.stringify({
                title: "已完成双优建设报告",
                section_id: "sec_1",
              }),
            },
            {
              tool_call_id: "tc_1",
              tool_name: "apply_operations",
              status: "completed",
              input_preview: JSON.stringify({ section_id: "sec_1" }),
            },
          ],
          subagents: [
            {
              subagent_run_id: "sub_1",
              agent_name: "researcher",
              display_name: "数据检索员",
              task: "检索数据",
              status: "completed",
              task_focus: {
                title: "完成数据核验",
                todos: [{ title: "完成数据核验", status: "completed" }],
              },
              segments: [],
            },
          ],
        },
      ],
    };

    const milestones = buildHistoricalMilestones(trace);
    expect(milestones).toHaveLength(2);
    expect(milestones[0]?.title).toBe("阶段一：采集经费来源");
    expect(milestones[0]?.semanticPills[0]?.label).toBe("登记数据来源");
    expect(milestones[1]?.title).toBe("已完成双优建设报告");
    expect(milestones[1]?.semanticPills[0]?.label).toBe("写入报告内容");

    const model = buildHistoricalRunStatusModel(trace);
    expect(model.status).toBe("completed");
    expect(model.title).toBe("已完成双优建设报告");
    expect(model.toolCount).toBe(2);
    expect(model.completedToolCount).toBe(2);
    expect(model.todos).toHaveLength(2);
    expect(model.todos.every((t) => t.status === "completed")).toBe(true);
    expect(model.subagents[0]?.activityText).toBe("完成数据核验");
    expect(model.elapsedLabel).toBe("14秒");

    const html = renderToStaticMarkup(
      <RunActivityHeader
        model={model}
        isStreaming={false}
        detailsOpen={false}
        onToggleDetails={() => {}}
        onJumpToSection={() => {}}
      />,
    );
    expect(html).toContain('data-live-header="true"');
    expect(html).toContain('data-status="completed"');
    expect(html).toContain('data-presentation="receipt"');
    expect(html).not.toContain('data-slot="agent-plan"');
    expect(html).not.toContain('data-slot="subagent-list"');
    expect(html).not.toContain('data-live-progress-tools="true"');
    expect(html).not.toContain('data-live-progress-subagents="true"');
    expect(html).not.toContain("完成数据核验");
    expect(html).toContain('data-live-header-jump="true"');
    expect(html).toContain("定位章节");
    expect(html.split("14秒").length - 1).toBe(1);

    const expandedHistorical = renderToStaticMarkup(
      <RunActivityHeader
        model={model}
        isStreaming={false}
        detailsOpen={true}
        onToggleDetails={() => {}}
      />,
    );
    expect(expandedHistorical).toContain("阶段一：采集经费来源");
    expect(expandedHistorical).toContain('data-slot="subagent-todo-list"');
    expect(expandedHistorical).toContain("完成数据核验");
    expect(expandedHistorical).not.toContain('data-slot="subagent-output"');
    expect(expandedHistorical).not.toContain(
      'data-slot="semantic-action-pills"',
    );

    const traceHtml = renderToStaticMarkup(
      <HistoricalRunTrace
        sessionId="sess_1"
        trace={trace}
        onJumpToSection={() => {}}
      />,
    );
    expect(traceHtml).toContain('data-presentation="receipt"');
    expect(traceHtml).not.toContain('data-slot="agent-plan"');
    expect(traceHtml).not.toContain("阶段一：采集经费来源");
    expect(traceHtml).not.toContain("历史内部思考文本同样不向终端用户展示");

    // Verify pure chat-session source extraction without a report document
    const richCtx = buildChatRichContext(null, [trace], null, []);
    const moeSource = richCtx.sources.get("moe-2024");
    expect(moeSource?.title).toBe("2024年全国教育事业发展统计公报");
    expect(moeSource?.publisher).toBe("中华人民共和国教育部");
    expect(moeSource?.excerpt).toBe("全国共有高等学校 3119 所。");
  });

  test("buildSemanticActionPills filters out low-signal plumbing tools and hides raw #block_ids", () => {
    const pills = buildSemanticActionPills([
      {
        toolCallId: "t1",
        toolName: "get_protocol_guide",
        status: "completed",
        inputPreview: "{}",
      },
      {
        toolCallId: "t2",
        toolName: "load_skill",
        status: "completed",
        inputPreview: JSON.stringify({ name: "higher-education-research" }),
      },
      {
        toolCallId: "t3",
        toolName: "read_blocks",
        status: "completed",
        inputPreview: JSON.stringify({ block_id: "c2-1" }),
      },
      {
        toolCallId: "t4",
        toolName: "panorama_report_universities",
        status: "completed",
        inputPreview: JSON.stringify({ university: "浙江大学" }),
      },
      {
        toolCallId: "t5",
        toolName: "apply_operations",
        status: "completed",
        inputPreview: JSON.stringify({ section_id: "c2-1" }),
      },
    ]);

    expect(pills.map((p) => p.label)).toEqual([
      "查询高教全景数据",
      "写入报告内容",
    ]);
    expect(pills[0]?.targets).toEqual(["浙江大学"]);
    expect(pills[1]?.targets).toEqual([]);
  });

  test("AgentAvatar renders deterministic mirrored Identicons without pulsing dots", () => {
    const mainHtml = renderToStaticMarkup(
      <AgentAvatar actor="main" active={true} />,
    );
    const analystHtml = renderToStaticMarkup(
      <AgentAvatar actor="analyst" active={true} />,
    );
    const writerHtml = renderToStaticMarkup(
      <AgentAvatar actor="writer" active={true} />,
    );
    expect(mainHtml).toContain("<svg");
    expect(mainHtml).not.toContain("animate-pulse");
    expect(
      renderToStaticMarkup(<AgentAvatar actor="main" active={true} />),
    ).toBe(mainHtml);
    const cells = Array.from(
      mainHtml.matchAll(/<rect x="(\d+)" y="(\d+)" width="6"/g),
      (match) => ({ x: Number(match[1]), y: Number(match[2]) }),
    );
    expect(cells.length).toBeGreaterThan(0);
    for (const cell of cells) {
      expect(
        cells.some(
          (candidate) => candidate.y === cell.y && candidate.x === 34 - cell.x,
        ),
      ).toBe(true);
    }
    expect(analystHtml).not.toEqual(writerHtml);
    expect(mainHtml).not.toEqual(analystHtml);
  });

  test("omits subagent subtitle when describe_task is absent and keeps latest describe_task for main and subagents", () => {
    const liveMain: LiveAgentTrace = {
      runId: "run_sub_subtitle",
      agentId: "main",
      isStreaming: true,
      taskFocus: { title: "主代理最新任务" },
      segments: [
        {
          kind: "execution",
          tools: [
            {
              toolCallId: "tc_m1",
              toolName: "describe_task",
              status: "completed",
              inputPreview: JSON.stringify({ title: "主代理早期任务" }),
            },
            {
              toolCallId: "tc_m2",
              toolName: "describe_task",
              status: "completed",
              inputPreview: JSON.stringify({ title: "主代理最新任务" }),
            },
          ],
          subagentRunIds: ["sub_no_desc", "sub_with_desc"],
        },
      ],
    };

    const liveSubagents: LiveSubagentRun[] = [
      {
        subagentRunId: "sub_no_desc",
        agentName: "designer",
        displayName: "版式专员",
        task: "调整版式",
        status: "running",
        trace: {
          agentId: "designer",
          isStreaming: true,
          segments: [
            {
              kind: "execution",
              tools: [
                {
                  toolCallId: "tc_d1",
                  toolName: "set_cover",
                  status: "running",
                  inputPreview: "{}",
                },
              ],
              subagentRunIds: [],
            },
          ],
        },
      },
      {
        subagentRunId: "sub_with_desc",
        agentName: "humanizer",
        displayName: "文字润色",
        task: "润色正文",
        status: "completed",
        trace: {
          agentId: "humanizer",
          isStreaming: false,
          taskFocus: { title: "子代理最新阶段" },
          segments: [
            {
              kind: "execution",
              tools: [
                {
                  toolCallId: "tc_h1",
                  toolName: "describe_task",
                  status: "completed",
                  inputPreview: JSON.stringify({ title: "子代理早期阶段" }),
                },
                {
                  toolCallId: "tc_h2",
                  toolName: "describe_task",
                  status: "completed",
                  inputPreview: JSON.stringify({ title: "子代理最新阶段" }),
                },
              ],
              subagentRunIds: [],
            },
          ],
        },
      },
    ];

    const liveModel = buildLiveRunStatusModel(liveMain, liveSubagents, 5);
    expect(liveModel.title).toBe("主代理最新任务");
    expect(liveModel.subagents[0]?.activityText).toBeUndefined();
    expect(liveModel.subagents[1]?.activityText).toBe("子代理最新阶段");

    const liveCollapsedHtml = renderToStaticMarkup(
      <RunActivityHeader
        model={liveModel}
        isStreaming={true}
        detailsOpen={false}
        onToggleDetails={() => {}}
      />,
    );
    expect(liveCollapsedHtml).not.toContain("协作子代理");
    expect(liveCollapsedHtml).not.toContain("协作执行中");
    expect(liveCollapsedHtml).toContain('title="版式专员 · 进行中"');
    expect(liveCollapsedHtml).toContain(
      'title="文字润色 · 已完成 · 子代理最新阶段"',
    );

    const liveExpandedHtml = renderToStaticMarkup(
      <RunActivityHeader
        model={liveModel}
        isStreaming={true}
        detailsOpen={true}
        onToggleDetails={() => {}}
      />,
    );
    expect(liveExpandedHtml).toContain("主代理最新任务");
    expect(liveExpandedHtml).toContain("子代理最新阶段");
    expect(liveExpandedHtml).not.toContain("协作子代理");
    expect(liveExpandedHtml).not.toContain("协作执行中");
    expect(liveExpandedHtml).not.toContain("正在设置封面");

    const historicalTrace: RunTraceSummary = {
      run_id: "run_hist_sub",
      status: "completed",
      task_focus: { title: "主代理最终阶段" },
      segments: [
        {
          kind: "execution",
          tools: [],
          subagents: [
            {
              subagent_run_id: "sub_hist_no_desc",
              agent_name: "designer",
              display_name: "版式专员",
              task: "调整版式",
              status: "completed",
              segments: [
                {
                  kind: "execution",
                  tools: [
                    {
                      tool_call_id: "tc_hd1",
                      tool_name: "set_cover",
                      status: "completed",
                      input_preview: "{}",
                    },
                  ],
                },
              ],
            },
            {
              subagent_run_id: "sub_hist_with_desc",
              agent_name: "researcher",
              display_name: "学科调研员",
              task: "核查指标",
              status: "completed",
              task_focus: { title: "完成学科指标核查" },
              segments: [],
            },
          ],
        },
      ],
    };

    const histModel = buildHistoricalRunStatusModel(historicalTrace);
    expect(histModel.title).toBe("主代理最终阶段");
    expect(histModel.subagents[0]?.activityText).toBeUndefined();
    expect(histModel.subagents[1]?.activityText).toBe("完成学科指标核查");

    const histExpandedHtml = renderToStaticMarkup(
      <RunActivityHeader
        model={histModel}
        isStreaming={false}
        detailsOpen={true}
        onToggleDetails={() => {}}
      />,
    );
    expect(histExpandedHtml).toContain("已完成主代理最终阶段");
    expect(histExpandedHtml).toContain(">完成学科指标核查</p>");
    expect(histExpandedHtml).not.toContain("协作子代理");
    expect(histExpandedHtml).not.toContain(">资料检索与核验</p>");
    expect(histExpandedHtml).not.toContain("设置封面");
    const noDescStart = histExpandedHtml.indexOf(
      'data-subagent-id="sub_hist_no_desc"',
    );
    const withDescStart = histExpandedHtml.indexOf(
      'data-subagent-id="sub_hist_with_desc"',
    );
    expect(noDescStart).toBeGreaterThan(-1);
    expect(withDescStart).toBeGreaterThan(noDescStart);
    expect(histExpandedHtml.slice(noDescStart, withDescStart)).not.toContain(
      "<p",
    );
  });

  test("separates live describe_task status from update_todos checklist for main and subagents", () => {
    const liveMain: LiveAgentTrace = {
      runId: "run_split",
      agentId: "main",
      isStreaming: true,
      taskFocus: {
        title: "核对省属高校国自然口径冲突",
        detail: "对比2021—2025年面上与青年项目增量",
        sectionId: "sec_1",
        todos: [
          { title: "立项与并行调研", status: "completed" },
          { title: "交叉补证与框架建桩", status: "in_progress" },
          { title: "派生测算与并行撰稿", status: "pending" },
        ],
      },
      segments: [
        {
          kind: "execution",
          tools: [
            {
              toolCallId: "tc_u1",
              toolName: "update_todos",
              status: "completed",
              inputPreview: JSON.stringify({
                todos: [
                  { title: "立项与并行调研", status: "completed" },
                  { title: "交叉补证与框架建桩", status: "in_progress" },
                  { title: "派生测算与并行撰稿", status: "pending" },
                ],
              }),
            },
            {
              toolCallId: "tc_d1",
              toolName: "describe_task",
              status: "completed",
              inputPreview: JSON.stringify({
                title: "核对省属高校国自然口径冲突",
                detail: "对比2021—2025年面上与青年项目增量",
                section_id: "sec_1",
              }),
            },
          ],
          subagentRunIds: ["sub_todos_only", "sub_both"],
        },
      ],
    };

    const subagents: LiveSubagentRun[] = [
      {
        subagentRunId: "sub_todos_only",
        agentName: "researcher",
        displayName: "调研员甲",
        task: "查核基金委口径",
        status: "running",
        trace: {
          runId: "sub_todos_only",
          agentId: "researcher",
          isStreaming: true,
          taskFocus: {
            title: "",
            todos: [{ title: "查核基金委口径", status: "in_progress" }],
          },
          segments: [
            {
              kind: "execution",
              tools: [
                {
                  toolCallId: "tc_su1",
                  toolName: "update_todos",
                  status: "completed",
                  inputPreview: JSON.stringify({
                    todos: [{ title: "查核基金委口径", status: "in_progress" }],
                  }),
                },
              ],
              subagentRunIds: [],
            },
          ],
        },
      },
      {
        subagentRunId: "sub_both",
        agentName: "analyst",
        displayName: "量化分析师",
        task: "测算五年增量",
        status: "running",
        trace: {
          runId: "sub_both",
          agentId: "analyst",
          isStreaming: true,
          taskFocus: {
            title: "绘制双轴增量对比图",
            todos: [
              { title: "汇总五年基准值", status: "completed" },
              { title: "绘制增量对比图表", status: "in_progress" },
            ],
          },
          segments: [],
        },
      },
    ];

    const liveModel = buildLiveRunStatusModel(liveMain, subagents, 12);
    expect(liveModel.title).toBe("核对省属高校国自然口径冲突");
    expect(liveModel.detail).toBe("对比2021—2025年面上与青年项目增量");
    expect(liveModel.todos).toHaveLength(3);
    expect(liveModel.todos[1]?.title).toBe("交叉补证与框架建桩");
    expect(liveModel.todos[1]?.status).toBe("in_progress");
    expect(liveModel.subagents[0]?.activityText).toBeUndefined();
    expect(liveModel.subagents[0]?.todos).toHaveLength(1);
    expect(liveModel.subagents[1]?.activityText).toBe("绘制双轴增量对比图");
    expect(liveModel.subagents[1]?.todos).toHaveLength(2);

    const collapsedHtml = renderToStaticMarkup(
      <RunActivityHeader
        model={liveModel}
        isStreaming={true}
        detailsOpen={false}
        onToggleDetails={() => {}}
      />,
    );
    expect(collapsedHtml).toContain("核对省属高校国自然口径冲突");
    expect(collapsedHtml).toContain("对比2021—2025年面上与青年项目增量");
    expect(collapsedHtml).toContain("交叉补证与框架建桩");

    const expandedHtml = renderToStaticMarkup(
      <RunActivityHeader
        model={liveModel}
        isStreaming={true}
        detailsOpen={true}
        onToggleDetails={() => {}}
      />,
    );
    expect(expandedHtml).toContain("查核基金委口径");
    expect(expandedHtml).toContain("绘制双轴增量对比图");
    const todosOnlyStart = expandedHtml.indexOf(
      'data-subagent-id="sub_todos_only"',
    );
    const bothStart = expandedHtml.indexOf('data-subagent-id="sub_both"');
    expect(todosOnlyStart).toBeGreaterThan(-1);
    expect(bothStart).toBeGreaterThan(todosOnlyStart);
    expect(expandedHtml.slice(todosOnlyStart, bothStart)).not.toContain("<p");
  });
});

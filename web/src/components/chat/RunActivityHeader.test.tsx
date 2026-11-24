import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { HistoricalRunTrace } from "@/components/chat/ExecutionTraceSection";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import {
  buildHistoricalMilestones,
  buildLiveMilestones,
} from "@/components/chat/taskMilestones";
import {
  buildHistoricalRunStatusModel,
  buildLiveRunStatusModel,
} from "@/components/chat/traceSegments";
import type { RunTraceSummary } from "@/contracts/session-protocol";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

describe("RunStatusModel, assistant-ui Elements & describe_task Milestones", () => {
  test("buildLiveRunStatusModel tracks both main agent and subagents cleanly with AgentPlan & SubagentList", () => {
    const liveMain: LiveAgentTrace = {
      runId: "run_1",
      agentId: "main",
      isStreaming: true,
      startedAtMs: 1_000,
      taskFocus: { title: "正在统筹撰写双优建设报告", detail: "核对高校数据" },
      segments: [
        {
          kind: "execution",
          tools: [
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
          taskFocus: { title: "检索国自然立项数据" },
          segments: [],
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
    expect(model.toolCount).toBe(2);
    expect(model.completedToolCount).toBe(1);
    expect(model.subagents).toHaveLength(1);
    expect(model.subagents[0]?.activityText).toBe("检索国自然立项数据");
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
    expect(html).toContain("主控");
    expect(html).toContain("第一阶段：构建报告框架");
    expect(html).toContain("正在统筹撰写双优建设报告");
    expect(html).toContain("1 of 2");
    expect(html).toContain('data-subagent-pill="sub_1"');
    expect(html).toContain("检索国自然立项数据");
  });

  test("buildHistoricalRunStatusModel preserves the exact same header structure after completion", () => {
    const trace: RunTraceSummary = {
      run_id: "run_1",
      status: "completed",
      duration_ms: 14_200,
      task_focus: { title: "已完成双优建设报告", section_id: "sec_1" },
      segments: [
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
              }),
            },
            {
              tool_call_id: "tc_s1",
              tool_name: "upsert_sources",
              status: "completed",
              input_preview: JSON.stringify({ title: "教育部经费公报" }),
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
              task_focus: { title: "完成数据核验" },
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
    expect(html).toContain('data-slot="agent-plan"');
    expect(html).toContain("2 of 2");
    expect(html).toContain("2 次工具");
    expect(html).toContain("1 个子代理");
    expect(html).toContain("完成数据核验");
    expect(html).toContain('data-live-header-jump="true"');

    const traceHtml = renderToStaticMarkup(
      <HistoricalRunTrace
        sessionId="sess_1"
        trace={trace}
        onJumpToSection={() => {}}
      />,
    );
    expect(traceHtml).toContain('data-slot="agent-plan"');
    expect(traceHtml).toContain("阶段一：采集经费来源");
  });
});

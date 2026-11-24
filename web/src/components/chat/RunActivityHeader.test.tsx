import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import {
  buildHistoricalRunStatusModel,
  buildLiveRunStatusModel,
} from "@/components/chat/traceSegments";
import type { RunTraceSummary } from "@/contracts/session-protocol";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

describe("RunStatusModel & RunActivityHeader", () => {
  test("buildLiveRunStatusModel tracks both main agent and subagents cleanly", () => {
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
              toolCallId: "tc_1",
              toolName: "apply_operations",
              status: "running",
              inputPreview: "{}",
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

    const model = buildLiveRunStatusModel(liveMain, subagents, 14);
    expect(model.status).toBe("running");
    expect(model.title).toBe("正在统筹撰写双优建设报告");
    expect(model.toolCount).toBe(1);
    expect(model.completedToolCount).toBe(0);
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
    expect(html).toContain("主控");
    expect(html).toContain("正在统筹撰写双优建设报告");
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
              tool_call_id: "tc_1",
              tool_name: "apply_operations",
              status: "completed",
              input_preview: "{}",
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

    const model = buildHistoricalRunStatusModel(trace);
    expect(model.status).toBe("completed");
    expect(model.title).toBe("已完成双优建设报告");
    expect(model.toolCount).toBe(1);
    expect(model.completedToolCount).toBe(1);
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
    expect(html).toContain("1 次工具");
    expect(html).toContain("1 个子代理");
    expect(html).toContain("完成数据核验");
    expect(html).toContain('data-live-header-jump="true"');
  });
});

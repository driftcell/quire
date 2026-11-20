import { describe, expect, test } from "bun:test";
import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
import type {
  RunTraceSubagent,
  RunTraceSummary,
} from "@/contracts/session-protocol";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";
import {
  buildHistoricalSegments,
  buildLiveSegments,
  extractSubagentLiveView,
  formatLiveElapsed,
  summarizeActivity,
  summarizeLiveProgress,
  type RenderSegment,
} from "./traceSegments";

function tool(id: string, toolName = "read_blocks"): ToolCallTraceItem {
  return {
    toolCallId: id,
    toolName,
    status: "completed",
    inputPreview: "{}",
  };
}

function subagent(id: string): RunTraceSubagent {
  return {
    subagent_run_id: id,
    agent_name: "writer",
    task: "write chapter",
    status: "completed",
    thinking: "",
  };
}

function segmentAt(segments: RenderSegment[], index: number): RenderSegment {
  const segment = segments[index];
  if (!segment) {
    throw new Error(`missing segment ${index}`);
  }
  return segment;
}

describe("summarizeActivity", () => {
  test("counts visible tools and concurrent running subagents", () => {
    const runningSub = {
      ...subagent("sub-1"),
      status: "running",
    };
    const summary = summarizeActivity([
      {
        key: "execution-1",
        kind: "execution",
        tools: [
          tool("read-1"),
          { ...tool("chart-1", "preview_echarts"), status: "running" },
        ],
        subagents: [
          { kind: "historical", key: "sub-1", subagent: runningSub },
          {
            kind: "historical",
            key: "sub-2",
            subagent: { ...subagent("sub-2"), status: "running" },
          },
        ],
      },
      {
        key: "thinking-1",
        kind: "thinking",
        text: "plan",
        isStreaming: true,
        isActive: true,
      },
    ]);

    expect(summary).toEqual({
      toolCount: 2,
      subagentCount: 2,
      runningSubagents: 2,
      runningTools: 1,
    });
  });
});

describe("live header progress", () => {
  test("counts visible main tools and settled subagents", () => {
    const trace: LiveAgentTrace = {
      agentId: "main",
      isStreaming: true,
      segments: [
        {
          kind: "execution",
          tools: [
            tool("read-1"),
            { ...tool("chart-1", "preview_echarts"), status: "running" },
            tool("spawn-1", "spawn_subagents"),
          ],
          subagentRunIds: ["sub-1", "sub-2"],
        },
      ],
    };
    const runningSubagent: LiveSubagentRun = {
      subagentRunId: "sub-1",
      agentName: "writer",
      task: "write",
      status: "running",
      trace: { agentId: "writer", isStreaming: true, segments: [] },
    };
    const completedSubagent: LiveSubagentRun = {
      ...runningSubagent,
      subagentRunId: "sub-2",
      status: "completed",
    };

    expect(
      summarizeLiveProgress(trace, [runningSubagent, completedSubagent]),
    ).toEqual({
      toolCount: 2,
      completedToolCount: 1,
      subagentCount: 2,
      completedSubagentCount: 1,
    });
  });

  test("formats elapsed time with second precision", () => {
    expect(formatLiveElapsed(0)).toBe("0秒");
    expect(formatLiveElapsed(12.9)).toBe("12秒");
    expect(formatLiveElapsed(60)).toBe("1分");
    expect(formatLiveElapsed(75)).toBe("1分15秒");
    expect(formatLiveElapsed(Number.NaN)).toBe("0秒");
  });
});

describe("buildHistoricalSegments", () => {
  test("groups process head and keeps the trailing body as text", () => {
    const trace: RunTraceSummary = {
      run_id: "run-1",
      status: "completed",
      duration_ms: 60000,
      segments: [
        { kind: "thinking", text: "   " },
        { kind: "thinking", text: "plan", duration_ms: 4000 },
        { kind: "execution", tools: [], subagents: [subagent("sub-1")] },
        { kind: "text", text: "progress" },
        { kind: "text", text: "final body" },
      ],
    };

    const segments = buildHistoricalSegments(trace);
    expect(segments).toHaveLength(3);
    expect(segmentAt(segments, 0).kind).toBe("activity");
    expect(segmentAt(segments, 1).kind).toBe("text");
    expect(segmentAt(segments, 2).kind).toBe("text");

    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    expect(activity.items).toHaveLength(2);
    expect(activity.isActive).toBe(false);
    expect(activity.durationMs).toBe(60000);
    const thinking = activity.items[0];
    if (thinking?.kind !== "thinking") {
      throw new Error("expected thinking item");
    }
    expect(thinking.durationMs).toBe(4000);
    const execution = activity.items[1];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution item");
    }
    expect(execution.subagents).toEqual([
      { kind: "historical", key: "sub-1", subagent: subagent("sub-1") },
    ]);

    const progress = segmentAt(segments, 1);
    if (progress.kind !== "text") {
      throw new Error("expected text segment");
    }
    expect(progress.text).toBe("progress");
    const body = segmentAt(segments, 2);
    if (body.kind !== "text") {
      throw new Error("expected text segment");
    }
    expect(body.text).toBe("final body");
    expect(body.isStreaming).toBe(false);
  });

  test("hides orchestration tools and keeps a subagent-only execution", () => {
    const trace: RunTraceSummary = {
      run_id: "run-2",
      status: "completed",
      segments: [
        {
          kind: "execution",
          tools: [
            {
              tool_call_id: "spawn-1",
              tool_name: "spawn_subagents",
              status: "completed",
              input_preview: "{}",
            },
            {
              tool_call_id: "describe-1",
              tool_name: "describe_task",
              status: "completed",
              input_preview: "{}",
            },
          ],
          subagents: [subagent("sub-1")],
        },
      ],
    };

    const segments = buildHistoricalSegments(trace);
    expect(segments).toHaveLength(1);
    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    expect(activity.items).toHaveLength(1);
    const execution = activity.items[0];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution item");
    }
    expect(execution.tools).toEqual([]);
    expect(execution.subagents).toEqual([
      { kind: "historical", key: "sub-1", subagent: subagent("sub-1") },
    ]);
  });

  test("keeps a failed spawn_subagents call visible", () => {
    const trace: RunTraceSummary = {
      run_id: "run-3",
      status: "failed",
      segments: [
        {
          kind: "execution",
          tools: [
            {
              tool_call_id: "spawn-1",
              tool_name: "spawn_subagents",
              status: "failed",
              input_preview: "{}",
            },
          ],
          subagents: [],
        },
      ],
    };

    const segments = buildHistoricalSegments(trace);
    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    const execution = activity.items[0];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution item");
    }
    expect(execution.tools.map((item) => item.toolCallId)).toEqual(["spawn-1"]);
    expect(execution.tools[0]?.status).toBe("failed");
  });
});

describe("buildLiveSegments", () => {
  test("resolves live subagents and marks the tail as streaming", () => {
    const liveTool = tool("call-1");
    const trace: LiveAgentTrace = {
      agentId: "main",
      isStreaming: true,
      segments: [
        { kind: "thinking", text: "plan", startedAtMs: 1000 },
        {
          kind: "execution",
          tools: [liveTool],
          subagentRunIds: ["sub-1", "missing"],
        },
        { kind: "text", text: "body" },
      ],
    };
    const liveSub: LiveSubagentRun = {
      subagentRunId: "sub-1",
      agentName: "writer",
      task: "write",
      status: "running",
      trace: {
        agentId: "writer",
        isStreaming: true,
        segments: [],
      },
    };

    const segments = buildLiveSegments(trace, [liveSub]);
    expect(segments).toHaveLength(2);
    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    expect(activity.isActive).toBe(true);
    const thinking = activity.items[0];
    if (thinking?.kind !== "thinking") {
      throw new Error("expected thinking item");
    }
    expect(thinking.startedAtMs).toBe(1000);
    const execution = activity.items[1];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution item");
    }
    expect(execution.tools).toEqual([liveTool]);
    expect(execution.subagents).toEqual([
      { kind: "live", key: "sub-1", subagent: liveSub },
    ]);
    const body = segmentAt(segments, 1);
    if (body.kind !== "text") {
      throw new Error("expected text segment");
    }
    expect(body.isStreaming).toBe(true);
  });

  test("settles activity while only the final body is streaming", () => {
    const trace: LiveAgentTrace = {
      agentId: "main",
      isStreaming: true,
      segments: [
        { kind: "thinking", text: "plan", durationMs: 1200 },
        {
          kind: "execution",
          tools: [tool("call-1")],
          subagentRunIds: [],
        },
        { kind: "text", text: "final body" },
      ],
    };

    const segments = buildLiveSegments(trace, []);
    expect(segments).toHaveLength(2);
    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    expect(activity.isActive).toBe(false);
    const body = segmentAt(segments, 1);
    if (body.kind !== "text") {
      throw new Error("expected text segment");
    }
    expect(body.isStreaming).toBe(true);
  });

  test("hides orchestration tools and keeps the live subagent card", () => {
    const trace: LiveAgentTrace = {
      agentId: "main",
      isStreaming: true,
      segments: [
        {
          kind: "execution",
          tools: [
            tool("spawn-1", "spawn_subagents"),
            tool("describe-1", "describe_task"),
            tool("read-1"),
          ],
          subagentRunIds: ["sub-1"],
        },
      ],
    };
    const liveSub: LiveSubagentRun = {
      subagentRunId: "sub-1",
      agentName: "writer",
      task: "write",
      status: "running",
      trace: {
        agentId: "writer",
        isStreaming: true,
        segments: [],
      },
    };

    const segments = buildLiveSegments(trace, [liveSub]);
    expect(segments).toHaveLength(1);
    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    const execution = activity.items[0];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution item");
    }
    expect(execution.tools.map((item) => item.toolCallId)).toEqual(["read-1"]);
    expect(execution.subagents).toEqual([
      { kind: "live", key: "sub-1", subagent: liveSub },
    ]);
  });

  test("keeps an execution segment when only subagents remain", () => {
    const trace: LiveAgentTrace = {
      agentId: "main",
      isStreaming: true,
      segments: [
        {
          kind: "execution",
          tools: [
            tool("spawn-1", "spawn_subagents"),
            tool("describe-1", "describe_task"),
          ],
          subagentRunIds: ["sub-1"],
        },
      ],
    };
    const liveSub: LiveSubagentRun = {
      subagentRunId: "sub-1",
      agentName: "writer",
      task: "write",
      status: "running",
      trace: {
        agentId: "writer",
        isStreaming: true,
        segments: [],
      },
    };

    const segments = buildLiveSegments(trace, [liveSub]);
    expect(segments).toHaveLength(1);
    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    const execution = activity.items[0];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution item");
    }
    expect(execution.tools).toEqual([]);
    expect(execution.subagents).toHaveLength(1);
  });

  test("keeps a failed live spawn_subagents call visible", () => {
    const trace: LiveAgentTrace = {
      agentId: "main",
      isStreaming: true,
      segments: [
        {
          kind: "execution",
          tools: [
            {
              ...tool("spawn-1", "spawn_subagents"),
              status: "failed",
            },
          ],
          subagentRunIds: [],
        },
      ],
    };

    const segments = buildLiveSegments(trace, []);
    const activity = segmentAt(segments, 0);
    if (activity.kind !== "activity") {
      throw new Error("expected activity segment");
    }
    const execution = activity.items[0];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution item");
    }
    expect(execution.tools.map((item) => item.toolCallId)).toEqual(["spawn-1"]);
    expect(execution.tools[0]?.status).toBe("failed");
  });
});

describe("inline user message boundaries", () => {
  test("historical trace closes activity before user and starts another after it", () => {
    const trace: RunTraceSummary = {
      run_id: "run-inline",
      status: "completed",
      segments: [
        { kind: "thinking", text: "first thought", duration_ms: 100 },
        {
          kind: "execution",
          tools: [
            {
              tool_call_id: "tool-1",
              tool_name: "read_blocks",
              status: "completed",
              input_preview: "{}",
            },
          ],
          subagents: [],
        },
        {
          kind: "user_message",
          message_id: "msg-inline",
          content: "补充风险提示",
          created_at: "2026-01-01T00:00:00Z",
        },
        { kind: "thinking", text: "second thought", duration_ms: 200 },
        {
          kind: "execution",
          tools: [
            {
              tool_call_id: "tool-2",
              tool_name: "read_blocks",
              status: "completed",
              input_preview: "{}",
            },
          ],
          subagents: [],
        },
        { kind: "text", text: "final answer" },
      ],
    };

    const segments = buildHistoricalSegments(trace);
    expect(segments.map((segment) => segment.kind)).toEqual([
      "activity",
      "user",
      "activity",
      "text",
    ]);
    const first = segmentAt(segments, 0);
    const user = segmentAt(segments, 1);
    const second = segmentAt(segments, 2);
    if (
      first.kind !== "activity" ||
      user.kind !== "user" ||
      second.kind !== "activity"
    ) {
      throw new Error("expected activity, user, activity");
    }
    expect(first.isActive).toBe(false);
    expect(first.items).toHaveLength(2);
    expect(user.messageId).toBe("msg-inline");
    expect(user.content).toBe("补充风险提示");
    expect(second.items).toHaveLength(2);
  });

  test("live trace applies the same user boundary", () => {
    const trace: LiveAgentTrace = {
      agentId: "main",
      runId: "run-inline",
      isStreaming: true,
      segments: [
        { kind: "thinking", text: "first thought", durationMs: 100 },
        {
          kind: "execution",
          tools: [tool("tool-1")],
          subagentRunIds: [],
        },
        {
          kind: "user_message",
          messageId: "msg-inline",
          content: "补充风险提示",
          createdAt: "2026-01-01T00:00:00Z",
        },
        { kind: "thinking", text: "second thought", durationMs: 200 },
        {
          kind: "execution",
          tools: [tool("tool-2")],
          subagentRunIds: [],
        },
        { kind: "text", text: "final answer" },
      ],
    };

    const segments = buildLiveSegments(trace, []);
    expect(segments.map((segment) => segment.kind)).toEqual([
      "activity",
      "user",
      "activity",
      "text",
    ]);
    const first = segmentAt(segments, 0);
    const second = segmentAt(segments, 2);
    if (first.kind !== "activity" || second.kind !== "activity") {
      throw new Error("expected two activity groups");
    }
    expect(first.isActive).toBe(false);
    expect(first.isLive).toBe(false);
    expect(first.items).toHaveLength(2);
    expect(second.items).toHaveLength(2);
    expect(segmentAt(segments, 3).kind).toBe("text");
  });
});

describe("extractSubagentLiveView", () => {
  test("keeps subagent output text out of the thinking/execution process", () => {
    const trace: LiveAgentTrace = {
      agentId: "writer",
      isStreaming: false,
      segments: [
        { kind: "thinking", text: "分析数据" },
        { kind: "text", text: "中间正文" },
        {
          kind: "execution",
          tools: [
            tool("read-1"),
            tool("describe-1", "describe_task"),
            tool("spawn-1", "spawn_subagents"),
            tool("read-2"),
          ],
          subagentRunIds: [],
        },
        { kind: "thinking", text: "复核口径" },
        { kind: "text", text: "最终正文" },
      ],
    };

    const view = extractSubagentLiveView(trace);
    expect(view.thinkingText).toBe("分析数据\n\n复核口径");
    expect(view.thinkingText).not.toContain("正文");
    expect(view.tools.map((item) => item.toolCallId)).toEqual([
      "read-1",
      "read-2",
    ]);
    expect(view.streamingText).toBe("中间正文\n\n最终正文");
    expect(view.isThinkingStreaming).toBe(false);
  });

  test("marks a trailing thinking segment as streaming", () => {
    const trace: LiveAgentTrace = {
      agentId: "writer",
      isStreaming: true,
      segments: [{ kind: "thinking", text: "正在推理" }],
    };

    const view = extractSubagentLiveView(trace);
    expect(view.thinkingText).toBe("正在推理");
    expect(view.isThinkingStreaming).toBe(true);
    expect(view.streamingText).toBe("");
    expect(view.tools).toEqual([]);
  });

  test("sums settled thinking durations and exposes the active start", () => {
    const trace: LiveAgentTrace = {
      agentId: "writer",
      isStreaming: true,
      segments: [
        { kind: "thinking", text: "第一段", durationMs: 2000 },
        { kind: "text", text: "中间输出" },
        { kind: "thinking", text: "第二段", startedAtMs: 5000 },
      ],
    };

    const view = extractSubagentLiveView(trace);
    expect(view.thinkingDurationMs).toBe(2000);
    expect(view.thinkingStartedAtMs).toBe(5000);
    expect(view.isThinkingStreaming).toBe(true);
  });
});

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

describe("buildHistoricalSegments", () => {
  test("groups process head and keeps the trailing body as text", () => {
    const trace: RunTraceSummary = {
      run_id: "run-1",
      status: "completed",
      duration_ms: 1200,
      segments: [
        { kind: "thinking", text: "   " },
        { kind: "thinking", text: "plan" },
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
    const thinking = activity.items[0];
    if (thinking?.kind !== "thinking") {
      throw new Error("expected thinking item");
    }
    expect(thinking.durationMs).toBe(1200);
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
});

describe("buildLiveSegments", () => {
  test("resolves live subagents and marks the tail as streaming", () => {
    const liveTool = tool("call-1");
    const trace: LiveAgentTrace = {
      agentId: "main",
      isStreaming: true,
      segments: [
        { kind: "thinking", text: "plan" },
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
});

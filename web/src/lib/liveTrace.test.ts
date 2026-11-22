import { describe, expect, test } from "bun:test";
import type {
  RunTraceSubagent,
  RunTraceSummary,
} from "@/contracts/session-protocol";
import {
  appendTextSegment,
  appendThinkingSegment,
  emptyLiveTrace,
  ensureLiveTraceStarted,
  finishThinkingSegment,
  hydrateActiveRun,
  liveSubagentFromSummary,
  liveTraceFromSummary,
  mergeHydratedSubagents,
} from "./liveTrace";

describe("live thinking timestamps", () => {
  test("keeps the server start and settles the active thinking duration", () => {
    let trace = emptyLiveTrace("main");
    trace = appendThinkingSegment(trace, "first", 1_000);
    trace = appendThinkingSegment(trace, " second", 1_500);

    const first = trace.segments[0];
    expect(first?.kind).toBe("thinking");
    if (first?.kind !== "thinking") {
      throw new Error("expected thinking segment");
    }
    expect(first.startedAtMs).toBe(1_000);
    expect(first.text).toBe("first second");

    trace = finishThinkingSegment(trace, 4_000);
    const settled = trace.segments[0];
    if (settled?.kind !== "thinking") {
      throw new Error("expected thinking segment");
    }
    expect(settled.durationMs).toBe(3_000);

    trace = appendThinkingSegment(trace, "third", 5_000);
    expect(trace.segments).toHaveLength(2);
    const second = trace.segments[1];
    if (second?.kind !== "thinking") {
      throw new Error("expected second thinking segment");
    }
    expect(second.startedAtMs).toBe(5_000);
  });

  test("keeps the turn_started clock across incremental events", () => {
    let trace = emptyLiveTrace("main", "run-1", 10_000);
    expect(trace.startedAtMs).toBe(10_000);

    trace = appendThinkingSegment(trace, "plan", 10_500);
    trace = appendTextSegment(finishThinkingSegment(trace, 11_000), "answer");
    expect(trace.startedAtMs).toBe(10_000);
  });

  test("fills a missing clock without overwriting an earlier timestamp", () => {
    let trace = emptyLiveTrace("main", "run-1");
    trace = ensureLiveTraceStarted(trace, 9_000);
    expect(trace.startedAtMs).toBe(9_000);

    trace = ensureLiveTraceStarted(trace, 12_000);
    expect(trace.startedAtMs).toBe(9_000);
  });

  test("ignores settlement after text has replaced the active segment", () => {
    let trace = emptyLiveTrace("main");
    trace = appendThinkingSegment(trace, "plan", 1_000);
    trace = appendTextSegment(finishThinkingSegment(trace, 2_000), "answer");
    trace = finishThinkingSegment(trace, 9_000);

    const text = trace.segments[1];
    expect(text?.kind).toBe("text");
    const thinking = trace.segments[0];
    if (thinking?.kind !== "thinking") {
      throw new Error("expected thinking segment");
    }
    expect(thinking.durationMs).toBe(1_000);
  });
});

describe("active run hydration", () => {
  const now = Date.now();
  const startedAt = new Date(now - 12_000).toISOString();
  const thinkingStartedAt = new Date(now - 8_000).toISOString();
  const streamingThinkingAt = new Date(now - 2_000).toISOString();
  const subagentThinkingStartedAt = new Date(now - 3_000).toISOString();

  const subagent: RunTraceSubagent = {
    subagent_run_id: "sub-1",
    agent_name: "analyst",
    display_name: "量化分析师",
    task: "核对统计口径",
    status: "running",
    started_at: new Date(now - 10_000).toISOString(),
    segments: [
      {
        kind: "thinking",
        text: "子代理正在核对数据。",
        started_at: subagentThinkingStartedAt,
      },
      {
        kind: "execution",
        tools: [
          {
            tool_call_id: "sub-tool",
            tool_name: "read_blocks",
            status: "completed",
            input_preview: "{}",
            output_preview: "{}",
            duration_ms: 500,
            truncated: false,
          },
        ],
        subagents: [],
      },
      { kind: "text", text: "子代理已返回部分结论。" },
    ],
    task_focus: { title: "核对统计口径" },
  };

  const trace: RunTraceSummary = {
    run_id: "run-active",
    status: "running",
    started_at: startedAt,
    task_focus: { title: "复核招生计划" },
    segments: [
      {
        kind: "thinking",
        text: "先核对统计口径。",
        duration_ms: 4_000,
        started_at: thinkingStartedAt,
      },
      {
        kind: "execution",
        tools: [
          {
            tool_call_id: "main-tool",
            tool_name: "read_blocks",
            status: "completed",
            input_preview: "{}",
            output_preview: "{}",
            duration_ms: 1_200,
            truncated: false,
          },
          {
            tool_call_id: "running-tool",
            tool_name: "preview_echarts",
            status: "running",
            input_preview: "{}",
            truncated: false,
          },
        ],
        subagents: [subagent],
      },
      {
        kind: "thinking",
        text: "正在继续思考。",
        started_at: streamingThinkingAt,
      },
    ],
  };

  test("restores the run clock, segments, tools and streaming thinking", () => {
    const live = liveTraceFromSummary(trace);

    expect(live.runId).toBe("run-active");
    expect(live.startedAtMs).toBe(Date.parse(startedAt));
    expect(live.isStreaming).toBe(true);
    expect(live.taskFocus).toEqual({ title: "复核招生计划" });

    const first = live.segments[0];
    if (first?.kind !== "thinking") {
      throw new Error("expected settled thinking");
    }
    expect(first.durationMs).toBe(4_000);
    expect(first.startedAtMs).toBe(Date.parse(thinkingStartedAt));

    const execution = live.segments[1];
    if (execution?.kind !== "execution") {
      throw new Error("expected execution segment");
    }
    expect(execution.tools.map((tool) => tool.toolCallId)).toEqual([
      "main-tool",
      "running-tool",
    ]);
    expect(execution.tools[0]?.status).toBe("completed");
    expect(execution.tools[1]?.status).toBe("running");
    expect(execution.subagentRunIds).toEqual(["sub-1"]);

    const streaming = live.segments[2];
    if (streaming?.kind !== "thinking") {
      throw new Error("expected streaming thinking");
    }
    expect(streaming.durationMs).toBeUndefined();
    expect(streaming.startedAtMs).toBe(Date.parse(streamingThinkingAt));
  });

  test("restores a running subagent with its own thinking clock and text", () => {
    const live = liveSubagentFromSummary(subagent);

    expect(live.subagentRunId).toBe("sub-1");
    expect(live.status).toBe("running");
    expect(live.trace.startedAtMs).toBe(Date.parse(subagent.started_at ?? ""));
    expect(live.trace.isStreaming).toBe(true);
    expect(live.trace.taskFocus).toEqual({ title: "核对统计口径" });

    const thinking = live.trace.segments[0];
    if (thinking?.kind !== "thinking") {
      throw new Error("expected subagent thinking");
    }
    expect(thinking.text).toBe("子代理正在核对数据。");
    expect(thinking.durationMs).toBeUndefined();
    expect(thinking.startedAtMs).toBe(Date.parse(subagentThinkingStartedAt));

    const execution = live.trace.segments[1];
    if (execution?.kind !== "execution") {
      throw new Error("expected subagent execution");
    }
    expect(execution.tools[0]?.toolCallId).toBe("sub-tool");

    expect(live.trace.segments[2]).toEqual({
      kind: "text",
      text: "子代理已返回部分结论。",
    });
  });

  test("hydrates the active run and its subagents from one snapshot", () => {
    const hydrated = hydrateActiveRun(trace, [subagent, subagent], true);

    expect(hydrated.main.runId).toBe("run-active");
    expect(hydrated.main.startedAtMs).toBe(Date.parse(startedAt));
    expect(hydrated.subagents).toHaveLength(1);
    expect(hydrated.subagents[0]?.subagentRunId).toBe("sub-1");
  });

  test("accepts a terminal snapshot for a locally running subagent", () => {
    const running = liveSubagentFromSummary(subagent);
    const completed = liveSubagentFromSummary({
      ...subagent,
      status: "completed",
      summary: "核对完成",
    });
    const failed = liveSubagentFromSummary({
      ...subagent,
      status: "failed",
      summary: "核对失败",
    });

    expect(mergeHydratedSubagents([running], [completed])[0]?.status).toBe(
      "completed",
    );
    expect(mergeHydratedSubagents([running], [failed])[0]?.status).toBe(
      "failed",
    );
    expect(mergeHydratedSubagents([completed], [running])[0]?.status).toBe(
      "completed",
    );
  });
});

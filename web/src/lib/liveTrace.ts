import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";

export type LiveTraceSegment =
  | {
      kind: "thinking";
      text: string;
      startedAtMs?: number;
      durationMs?: number;
    }
  | {
      kind: "execution";
      tools: ToolCallTraceItem[];
      subagentRunIds: string[];
    }
  | {
      kind: "user_message";
      messageId: string;
      content: string;
      createdAt: string;
    }
  | { kind: "text"; text: string };

export interface LiveAgentTrace {
  agentId: string;
  runId?: string;
  /** Server `turn_started` timestamp used as the stable run clock across refreshes. */
  startedAtMs?: number;
  segments: LiveTraceSegment[];
  taskFocus?: { title: string; detail?: string; sectionId?: string };
  isStreaming: boolean;
}

export interface LiveSubagentRun {
  subagentRunId: string;
  agentName: string;
  displayName?: string;
  task: string;
  sectionId?: string;
  status: "running" | "completed" | "failed";
  summary?: string;
  trace: LiveAgentTrace;
}

export function emptyLiveTrace(
  agentId: string,
  runId?: string,
  startedAtMs?: number,
): LiveAgentTrace {
  return {
    agentId,
    runId,
    startedAtMs,
    segments: [],
    isStreaming: true,
  };
}

/** Fill a missing run clock without replacing an earlier server timestamp. */
export function ensureLiveTraceStarted(
  trace: LiveAgentTrace,
  startedAtMs?: number,
): LiveAgentTrace {
  if (trace.startedAtMs !== undefined || startedAtMs === undefined) {
    return trace;
  }
  return { ...trace, startedAtMs };
}

function replaceLast(
  segments: LiveTraceSegment[],
  segment: LiveTraceSegment,
): LiveTraceSegment[] {
  return [...segments.slice(0, -1), segment];
}

export function appendThinkingSegment(
  trace: LiveAgentTrace,
  delta: string,
  startedAtMs?: number,
): LiveAgentTrace {
  const last = trace.segments[trace.segments.length - 1];
  if (last?.kind === "thinking" && last.durationMs === undefined) {
    return {
      ...trace,
      segments: replaceLast(trace.segments, {
        ...last,
        text: last.text + delta,
        startedAtMs: last.startedAtMs ?? startedAtMs,
      }),
    };
  }
  return {
    ...trace,
    segments: [
      ...trace.segments,
      { kind: "thinking", text: delta, startedAtMs },
    ],
  };
}

/** Settle the active trailing thinking segment with a server timestamp. */
export function finishThinkingSegment(
  trace: LiveAgentTrace,
  atMs: number,
): LiveAgentTrace {
  const last = trace.segments[trace.segments.length - 1];
  if (
    last?.kind !== "thinking" ||
    last.durationMs !== undefined ||
    last.startedAtMs === undefined
  ) {
    return trace;
  }
  return {
    ...trace,
    segments: replaceLast(trace.segments, {
      ...last,
      durationMs: Math.max(0, atMs - last.startedAtMs),
    }),
  };
}

export function appendUserMessageSegment(
  trace: LiveAgentTrace,
  messageId: string,
  content: string,
  createdAt: string,
): LiveAgentTrace {
  if (
    trace.segments.some(
      (segment) =>
        segment.kind === "user_message" && segment.messageId === messageId,
    )
  ) {
    return trace;
  }
  return {
    ...trace,
    segments: [
      ...trace.segments,
      { kind: "user_message", messageId, content, createdAt },
    ],
  };
}

export function appendTextSegment(
  trace: LiveAgentTrace,
  delta: string,
): LiveAgentTrace {
  const last = trace.segments[trace.segments.length - 1];
  if (last?.kind === "text") {
    return {
      ...trace,
      segments: replaceLast(trace.segments, {
        kind: "text",
        text: last.text + delta,
      }),
    };
  }
  return {
    ...trace,
    segments: [...trace.segments, { kind: "text", text: delta }],
  };
}

export function appendToolSegment(
  trace: LiveAgentTrace,
  tool: ToolCallTraceItem,
): LiveAgentTrace {
  const last = trace.segments[trace.segments.length - 1];
  if (last?.kind === "execution") {
    return {
      ...trace,
      segments: replaceLast(trace.segments, {
        ...last,
        tools: [...last.tools, tool],
      }),
    };
  }
  return {
    ...trace,
    segments: [
      ...trace.segments,
      { kind: "execution", tools: [tool], subagentRunIds: [] },
    ],
  };
}

export function appendSubagentSegment(
  trace: LiveAgentTrace,
  subagentRunId: string,
): LiveAgentTrace {
  const last = trace.segments[trace.segments.length - 1];
  if (last?.kind === "execution") {
    if (last.subagentRunIds.includes(subagentRunId)) {
      return trace;
    }
    return {
      ...trace,
      segments: replaceLast(trace.segments, {
        ...last,
        subagentRunIds: [...last.subagentRunIds, subagentRunId],
      }),
    };
  }
  return {
    ...trace,
    segments: [
      ...trace.segments,
      { kind: "execution", tools: [], subagentRunIds: [subagentRunId] },
    ],
  };
}

export interface ToolFinishPatch {
  status: ToolCallTraceItem["status"];
  outputPreview?: string;
  durationMs?: number;
}

export function finishToolSegment(
  trace: LiveAgentTrace,
  toolCallId: string,
  patch: ToolFinishPatch,
): LiveAgentTrace {
  for (let index = trace.segments.length - 1; index >= 0; index -= 1) {
    const segment = trace.segments[index];
    if (segment?.kind !== "execution") {
      continue;
    }
    const toolIndex = segment.tools.findLastIndex(
      (tool) => tool.toolCallId === toolCallId,
    );
    if (toolIndex === -1) {
      continue;
    }
    const tools = segment.tools.map((tool, current) =>
      current === toolIndex
        ? {
            ...tool,
            status: patch.status,
            outputPreview: patch.outputPreview,
            durationMs: patch.durationMs,
          }
        : tool,
    );
    const segments = [...trace.segments];
    segments[index] = { ...segment, tools };
    return { ...trace, segments };
  }
  return trace;
}

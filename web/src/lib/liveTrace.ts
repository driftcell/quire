import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";

export type LiveTraceSegment =
  | { kind: "thinking"; text: string }
  | {
      kind: "execution";
      tools: ToolCallTraceItem[];
      subagentRunIds: string[];
    }
  | { kind: "text"; text: string };

export interface LiveAgentTrace {
  agentId: string;
  segments: LiveTraceSegment[];
  taskFocus?: { title: string; detail?: string; sectionId?: string };
  isStreaming: boolean;
}

export interface LiveSubagentRun {
  subagentRunId: string;
  agentName: string;
  task: string;
  sectionId?: string;
  status: "running" | "completed" | "failed";
  summary?: string;
  trace: LiveAgentTrace;
}

export function emptyLiveTrace(agentId: string): LiveAgentTrace {
  return {
    agentId,
    segments: [],
    isStreaming: true,
  };
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
): LiveAgentTrace {
  const last = trace.segments[trace.segments.length - 1];
  if (last?.kind === "thinking") {
    return {
      ...trace,
      segments: replaceLast(trace.segments, {
        kind: "thinking",
        text: last.text + delta,
      }),
    };
  }
  return {
    ...trace,
    segments: [...trace.segments, { kind: "thinking", text: delta }],
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

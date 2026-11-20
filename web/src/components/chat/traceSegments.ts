import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
import { isVisibleTraceTool } from "@/components/chat/toolMeta";
import type {
  RunTraceSubagent,
  RunTraceSummary,
  RunTraceToolItem,
} from "@/contracts/session-protocol";
import type {
  LiveAgentTrace,
  LiveSubagentRun,
  LiveTraceSegment,
} from "@/lib/liveTrace";

export type ThinkingActivity = {
  key: string;
  kind: "thinking";
  text: string;
  isStreaming: boolean;
  isActive: boolean;
  startedAtMs?: number;
  durationMs?: number;
};

export type ExecutionSubagent =
  | { kind: "historical"; key: string; subagent: RunTraceSubagent }
  | { kind: "live"; key: string; subagent: LiveSubagentRun };

export type ExecutionActivity = {
  key: string;
  kind: "execution";
  tools: ToolCallTraceItem[];
  subagents: ExecutionSubagent[];
};

export type TextActivity = {
  key: string;
  kind: "text";
  text: string;
  isStreaming: boolean;
};

export type ActivityItem = ThinkingActivity | ExecutionActivity | TextActivity;

export type ActivityRenderSegment = {
  key: string;
  kind: "activity";
  items: ActivityItem[];
  isActive: boolean;
  durationMs?: number;
};

type TextRenderSegment = {
  key: string;
  kind: "text";
  text: string;
  isStreaming: boolean;
};

export type RenderSegment = ActivityRenderSegment | TextRenderSegment;

export type LiveToolTraceItem = ToolCallTraceItem;

export interface SubagentLiveView {
  thinkingText: string;
  isThinkingStreaming: boolean;
  thinkingDurationMs?: number;
  thinkingStartedAtMs?: number;
  tools: LiveToolTraceItem[];
  streamingText: string;
}

export interface ActivitySummary {
  toolCount: number;
  subagentCount: number;
  runningSubagents: number;
  runningTools: number;
}

export function summarizeActivity(items: ActivityItem[]): ActivitySummary {
  const tools = items.flatMap((item) =>
    item.kind === "execution" ? item.tools : [],
  );
  const subagents = items.flatMap((item) =>
    item.kind === "execution" ? item.subagents : [],
  );
  return {
    toolCount: tools.length,
    subagentCount: subagents.length,
    runningSubagents: subagents.filter(
      (entry) => entry.subagent.status === "running",
    ).length,
    runningTools: tools.filter((tool) => tool.status === "running").length,
  };
}

function joinLiveText(parts: string[]): string {
  return parts
    .filter((part) => part.trim().length > 0)
    .join("\n\n")
    .trim();
}

/**
 * Extract the subagent card model directly from the live trace. Subagent text
 * is its own output, never part of the "thinking and execution" process block.
 */
export function extractSubagentLiveView(
  trace: LiveAgentTrace,
): SubagentLiveView {
  const thinkingParts: string[] = [];
  const textParts: string[] = [];
  const tools: LiveToolTraceItem[] = [];
  let thinkingDurationMs = 0;
  let thinkingStartedAtMs: number | undefined;

  for (const segment of trace.segments) {
    if (segment.kind === "thinking") {
      thinkingParts.push(segment.text);
      if (segment.durationMs !== undefined) {
        thinkingDurationMs += segment.durationMs;
      } else {
        thinkingStartedAtMs = segment.startedAtMs;
      }
      continue;
    }
    if (segment.kind === "text") {
      textParts.push(segment.text);
      continue;
    }
    for (const tool of segment.tools) {
      if (isVisibleTraceTool(tool)) {
        tools.push(tool);
      }
    }
  }

  const lastSegment = trace.segments[trace.segments.length - 1];
  return {
    thinkingText: joinLiveText(thinkingParts),
    isThinkingStreaming: trace.isStreaming && lastSegment?.kind === "thinking",
    thinkingDurationMs: thinkingDurationMs > 0 ? thinkingDurationMs : undefined,
    thinkingStartedAtMs,
    tools,
    streamingText: joinLiveText(textParts),
  };
}

function traceToolStatus(status: string): ToolCallTraceItem["status"] {
  if (status === "running") {
    return "running";
  }
  if (status === "failed" || status === "error") {
    return "failed";
  }
  return "completed";
}

export function toToolTraceItems(
  tools: RunTraceToolItem[] | undefined,
): ToolCallTraceItem[] {
  return (tools ?? []).map((tool) => ({
    toolCallId: tool.tool_call_id,
    toolName: tool.tool_name,
    status: traceToolStatus(tool.status),
    inputPreview: tool.input_preview,
    outputPreview: tool.output_preview ?? undefined,
    truncated: tool.truncated ?? false,
    durationMs: tool.duration_ms ?? undefined,
  }));
}

export function activityHasQuestion(
  items: ActivityItem[],
  questionSlots?: ReadonlyMap<string, unknown>,
): boolean {
  if (!questionSlots) {
    return false;
  }
  return items.some(
    (item) =>
      item.kind === "execution" &&
      item.tools.some((tool) => questionSlots.has(tool.toolCallId)),
  );
}

export function buildHistoricalSegments(
  trace: RunTraceSummary,
): RenderSegment[] {
  const source = trace.segments ?? [];
  // The trailing text run is the run's visible body. Everything before it is
  // process: thinking, tool calls and any commentary the model emitted between
  // them. Keeping that process in one group makes the text the only divider.
  let tailStart = source.length;
  while (tailStart > 0 && source[tailStart - 1]?.kind === "text") {
    tailStart -= 1;
  }
  const head = source.slice(0, tailStart);
  const tail = source.slice(tailStart);

  const segments: RenderSegment[] = [];
  const activity: ActivityItem[] = [];
  head.forEach((segment, index) => {
    if (segment.kind === "thinking") {
      if (!segment.text.trim()) {
        return;
      }
      activity.push({
        key: `thinking-${index}`,
        kind: "thinking",
        text: segment.text,
        isStreaming: false,
        isActive: false,
        durationMs: segment.duration_ms ?? undefined,
      });
      return;
    }
    if (segment.kind === "text") {
      if (!segment.text.trim()) {
        return;
      }
      activity.push({
        key: `text-${index}`,
        kind: "text",
        text: segment.text,
        isStreaming: false,
      });
      return;
    }
    const tools = toToolTraceItems(segment.tools).filter(isVisibleTraceTool);
    const subagents = segment.subagents ?? [];
    if (tools.length === 0 && subagents.length === 0) {
      return;
    }
    activity.push({
      key: `execution-${index}`,
      kind: "execution",
      tools,
      subagents: subagents.map((subagent) => ({
        kind: "historical" as const,
        key: subagent.subagent_run_id,
        subagent,
      })),
    });
  });
  if (activity.length > 0) {
    const hasRunningTool = activity.some(
      (item) =>
        item.kind === "execution" &&
        item.tools.some((tool) => tool.status === "running"),
    );
    const hasRunningSubagent = activity.some(
      (item) =>
        item.kind === "execution" &&
        item.subagents.some((entry) => entry.subagent.status === "running"),
    );
    segments.push({
      key: `activity-${activity[0]?.key ?? segments.length}`,
      kind: "activity",
      items: activity,
      isActive:
        trace.status === "running" || hasRunningTool || hasRunningSubagent,
      durationMs: trace.duration_ms ?? undefined,
    });
  }
  tail.forEach((segment, index) => {
    if (segment.kind !== "text" || !segment.text.trim()) {
      return;
    }
    segments.push({
      key: `text-${tailStart + index}`,
      kind: "text",
      text: segment.text,
      isStreaming: false,
    });
  });
  return segments;
}

export function buildLiveSegments(
  trace: LiveAgentTrace,
  subagents: LiveSubagentRun[],
): RenderSegment[] {
  const lastIndex = trace.segments.length - 1;
  let tailStart = trace.segments.length;
  while (tailStart > 0 && trace.segments[tailStart - 1]?.kind === "text") {
    tailStart -= 1;
  }
  const head = trace.segments.slice(0, tailStart);
  const tail = trace.segments.slice(tailStart);

  const segments: RenderSegment[] = [];
  const activity: ActivityItem[] = [];
  head.forEach((segment: LiveTraceSegment, index) => {
    const isLast = index === lastIndex;
    if (segment.kind === "thinking") {
      if (!segment.text.trim()) {
        return;
      }
      activity.push({
        key: `thinking-${index}`,
        kind: "thinking",
        text: segment.text,
        isStreaming: trace.isStreaming && isLast,
        isActive: trace.isStreaming && isLast,
        startedAtMs: segment.startedAtMs,
        durationMs: segment.durationMs,
      });
      return;
    }
    if (segment.kind === "text") {
      if (!segment.text.trim()) {
        return;
      }
      activity.push({
        key: `text-${index}`,
        kind: "text",
        text: segment.text,
        isStreaming: false,
      });
      return;
    }
    const runs = segment.subagentRunIds
      .map((id) => subagents.find((sub) => sub.subagentRunId === id))
      .filter((sub): sub is LiveSubagentRun => sub !== undefined);
    const tools = segment.tools.filter(isVisibleTraceTool);
    if (tools.length === 0 && runs.length === 0) {
      return;
    }
    activity.push({
      key: `execution-${index}`,
      kind: "execution",
      tools,
      subagents: runs.map((subagent) => ({
        kind: "live" as const,
        key: subagent.subagentRunId,
        subagent,
      })),
    });
  });
  if (activity.length > 0) {
    const summary = summarizeActivity(activity);
    segments.push({
      key: `activity-${activity[0]?.key ?? segments.length}`,
      kind: "activity",
      items: activity,
      isActive:
        trace.isStreaming ||
        summary.runningSubagents > 0 ||
        summary.runningTools > 0,
    });
  }
  tail.forEach((segment, index) => {
    if (segment.kind !== "text" || !segment.text.trim()) {
      return;
    }
    segments.push({
      key: `text-${tailStart + index}`,
      kind: "text",
      text: segment.text,
      isStreaming: trace.isStreaming,
    });
  });
  return segments;
}

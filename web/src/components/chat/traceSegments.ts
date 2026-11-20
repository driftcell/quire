import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
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
  defaultOpen?: boolean;
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
  defaultOpen: boolean;
};

export type TextActivity = {
  key: string;
  kind: "text";
  text: string;
  isStreaming: boolean;
};

type ActivityItem = ThinkingActivity | ExecutionActivity | TextActivity;

export type ActivityRenderSegment = {
  key: string;
  kind: "activity";
  items: ActivityItem[];
};

type TextRenderSegment = {
  key: string;
  kind: "text";
  text: string;
  isStreaming: boolean;
};

export type RenderSegment = ActivityRenderSegment | TextRenderSegment;

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
  const lastThinkingIndex = source.reduce(
    (last, segment, index) => (segment.kind === "thinking" ? index : last),
    -1,
  );
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
        durationMs:
          index === lastThinkingIndex
            ? (trace.duration_ms ?? undefined)
            : undefined,
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
    const tools = toToolTraceItems(segment.tools);
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
      defaultOpen: false,
    });
  });
  if (activity.length > 0) {
    segments.push({
      key: `activity-${activity[0]?.key ?? segments.length}`,
      kind: "activity",
      items: activity,
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
  options?: { defaultCollapsed?: boolean },
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
        defaultOpen: options?.defaultCollapsed ? false : undefined,
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
    if (segment.tools.length === 0 && runs.length === 0) {
      return;
    }
    activity.push({
      key: `execution-${index}`,
      kind: "execution",
      tools: segment.tools,
      subagents: runs.map((subagent) => ({
        kind: "live" as const,
        key: subagent.subagentRunId,
        subagent,
      })),
      defaultOpen: !options?.defaultCollapsed,
    });
  });
  if (activity.length > 0) {
    segments.push({
      key: `activity-${activity[0]?.key ?? segments.length}`,
      kind: "activity",
      items: activity,
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

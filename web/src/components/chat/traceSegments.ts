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

export type ActivityItem = ThinkingActivity | ExecutionActivity;

export type ActivityRenderSegment = {
  key: string;
  kind: "activity";
  items: ActivityItem[];
  isActive: boolean;
  isLive?: boolean;
  durationMs?: number;
};

type TextRenderSegment = {
  key: string;
  kind: "text";
  text: string;
  isStreaming: boolean;
};

export type UserRenderSegment = {
  key: string;
  kind: "user";
  messageId: string;
  content: string;
  createdAt: string;
};

export type RenderSegment =
  ActivityRenderSegment | UserRenderSegment | TextRenderSegment;

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

export interface LiveProgressSummary {
  toolCount: number;
  completedToolCount: number;
  subagentCount: number;
  completedSubagentCount: number;
}

/**
 * Aggregate the live run's visible main-agent tools and subagent lifecycle.
 * Subagent tool calls stay inside their own cards so the header never double-counts them.
 */
export function summarizeLiveProgress(
  trace: LiveAgentTrace | null | undefined,
  subagents: LiveSubagentRun[],
): LiveProgressSummary {
  const tools = (trace?.segments ?? []).flatMap((segment) =>
    segment.kind === "execution"
      ? segment.tools.filter(isVisibleTraceTool)
      : [],
  );
  return {
    toolCount: tools.length,
    completedToolCount: tools.filter((tool) => tool.status !== "running")
      .length,
    subagentCount: subagents.length,
    completedSubagentCount: subagents.filter(
      (subagent) => subagent.status !== "running",
    ).length,
  };
}

/** Format an elapsed live-run duration with second-level precision. */
export function formatLiveElapsed(totalSeconds: number): string {
  const seconds = Math.max(
    0,
    Math.floor(Number.isFinite(totalSeconds) ? totalSeconds : 0),
  );
  if (seconds < 60) {
    return `${seconds}秒`;
  }
  const mins = Math.floor(seconds / 60);
  const remaining = seconds % 60;
  return remaining > 0 ? `${mins}分${remaining}秒` : `${mins}分`;
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
    if (segment.kind === "user_message") {
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
  const segments: RenderSegment[] = [];
  const activity: ActivityItem[] = [];
  const flushActivity = (isFinal: boolean) => {
    if (activity.length === 0) {
      return;
    }
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
      items: [...activity],
      isActive:
        isFinal &&
        (trace.status === "running" || hasRunningTool || hasRunningSubagent),
      durationMs: isFinal ? (trace.duration_ms ?? undefined) : undefined,
    });
    activity.length = 0;
  };

  source.forEach((segment, index) => {
    if (segment.kind === "user_message") {
      flushActivity(false);
      segments.push({
        key: `user-${segment.message_id}`,
        kind: "user",
        messageId: segment.message_id,
        content: segment.content,
        createdAt: segment.created_at,
      });
      return;
    }
    if (segment.kind === "text") {
      if (!segment.text.trim()) {
        return;
      }
      flushActivity(false);
      segments.push({
        key: `text-${index}`,
        kind: "text",
        text: segment.text,
        isStreaming: false,
      });
      return;
    }
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
  flushActivity(true);
  return segments;
}

export function buildLiveSegments(
  trace: LiveAgentTrace,
  subagents: LiveSubagentRun[],
): RenderSegment[] {
  const lastIndex = trace.segments.length - 1;
  const segments: RenderSegment[] = [];
  const activity: ActivityItem[] = [];
  const flushActivity = (isFinal: boolean) => {
    if (activity.length === 0) {
      return;
    }
    segments.push({
      key: `activity-${activity[0]?.key ?? segments.length}`,
      kind: "activity",
      items: [...activity],
      // The trailing activity stays active for the whole model-turn gap until
      // a text/user boundary or turn completion closes it. The top live header
      // owns the running state.
      isActive: isFinal && trace.isStreaming,
      isLive: isFinal ? trace.isStreaming : false,
    });
    activity.length = 0;
  };

  trace.segments.forEach((segment: LiveTraceSegment, index) => {
    if (segment.kind === "user_message") {
      flushActivity(false);
      segments.push({
        key: `user-${segment.messageId}`,
        kind: "user",
        messageId: segment.messageId,
        content: segment.content,
        createdAt: segment.createdAt,
      });
      return;
    }
    if (segment.kind === "text") {
      if (!segment.text.trim()) {
        return;
      }
      flushActivity(false);
      segments.push({
        key: `text-${index}`,
        kind: "text",
        text: segment.text,
        isStreaming: trace.isStreaming && index === lastIndex,
      });
      return;
    }
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
  flushActivity(true);
  return segments;
}

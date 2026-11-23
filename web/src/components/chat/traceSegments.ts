import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
import { isVisibleTraceTool } from "@/components/chat/toolMeta";
import type {
  RunTraceSubagent,
  RunTraceSummary,
} from "@/contracts/session-protocol";
import {
  toToolTraceItems,
  type LiveAgentTrace,
  type LiveSubagentRun,
  type LiveTraceSegment,
} from "@/lib/liveTrace";

export { toToolTraceItems };

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

export type RenderSegment = ActivityRenderSegment | TextRenderSegment;

export type LiveToolTraceItem = ToolCallTraceItem;

export interface SubagentLiveView {
  thinkingText: string;
  isThinkingStreaming: boolean;
  thinkingDurationMs?: number;
  thinkingStartedAtMs?: number;
  tools: LiveToolTraceItem[];
  streamingText: string;
  isTextStreaming: boolean;
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
    isTextStreaming: trace.isStreaming && lastSegment?.kind === "text",
  };
}

export function activityHasQuestion(
  items: ActivityItem[],
  questionSlots?: Map<string, unknown>,
): boolean {
  if (!questionSlots || questionSlots.size === 0) {
    return false;
  }
  return items.some(
    (item) =>
      item.kind === "execution" &&
      item.tools.some((tool) => questionSlots.has(tool.toolCallId)),
  );
}

/**
 * Group a historical run into strictly chronological activity and text
 * segments. A text segment closes the current activity group; later thinking
 * or tool calls open a new activity segment after that text.
 */
export function buildHistoricalSegments(
  trace: RunTraceSummary,
): RenderSegment[] {
  const result: RenderSegment[] = [];
  let pendingItems: ActivityItem[] = [];
  let activityGroupIndex = 0;

  const flushActivity = () => {
    if (pendingItems.length === 0) {
      return;
    }
    result.push({
      key: `${trace.run_id}-activity-${activityGroupIndex}`,
      kind: "activity",
      items: pendingItems,
      isActive: false,
      isLive: false,
    });
    pendingItems = [];
    activityGroupIndex += 1;
  };

  const source = trace.segments ?? [];
  source.forEach((segment, index) => {
    const key = `${trace.run_id}-seg-${index}`;
    if (segment.kind === "thinking") {
      const text = segment.text.trim();
      if (!text) {
        return;
      }
      pendingItems.push({
        key,
        kind: "thinking",
        text,
        isStreaming: false,
        isActive: false,
        durationMs: segment.duration_ms ?? undefined,
      });
      return;
    }

    if (segment.kind === "execution") {
      const tools = toToolTraceItems(segment.tools).filter(isVisibleTraceTool);
      const subagents: ExecutionSubagent[] = (segment.subagents ?? []).map(
        (sub) => ({
          kind: "historical",
          key: sub.subagent_run_id,
          subagent: sub,
        }),
      );
      if (tools.length === 0 && subagents.length === 0) {
        return;
      }
      pendingItems.push({
        key,
        kind: "execution",
        tools,
        subagents,
      });
      return;
    }

    const text = segment.text.trim();
    if (!text) {
      return;
    }
    flushActivity();
    result.push({
      key,
      kind: "text",
      text,
      isStreaming: false,
    });
  });

  flushActivity();
  return result;
}

function isLiveSegmentActive(
  segment: LiveTraceSegment,
  isLastSegment: boolean,
  isRunStreaming: boolean,
  subagentById: Map<string, LiveSubagentRun>,
): boolean {
  if (!isRunStreaming) {
    return false;
  }
  if (segment.kind === "thinking") {
    return isLastSegment;
  }
  if (segment.kind === "execution") {
    const hasRunningTool = segment.tools.some(
      (tool) => tool.status === "running",
    );
    const hasRunningSubagent = segment.subagentRunIds.some(
      (id) => subagentById.get(id)?.status === "running",
    );
    return isLastSegment || hasRunningTool || hasRunningSubagent;
  }
  return false;
}

/**
 * Group the live main-agent trace into chronological activity and text
 * segments. Only the trailing segment can stream; once a text segment is
 * emitted, any subsequent thinking or tool call opens a new activity segment.
 */
export function buildLiveSegments(
  trace: LiveAgentTrace,
  subagents: LiveSubagentRun[],
): RenderSegment[] {
  const subagentById = new Map(
    subagents.map((subagent) => [subagent.subagentRunId, subagent]),
  );
  const result: RenderSegment[] = [];
  let pendingItems: ActivityItem[] = [];
  let pendingIsActive = false;
  let activityGroupIndex = 0;

  const flushActivity = () => {
    if (pendingItems.length === 0) {
      return;
    }
    result.push({
      key: `live-activity-${activityGroupIndex}`,
      kind: "activity",
      items: pendingItems,
      isActive: pendingIsActive,
      isLive: pendingIsActive,
    });
    pendingItems = [];
    pendingIsActive = false;
    activityGroupIndex += 1;
  };

  trace.segments.forEach((segment, index) => {
    const key = `live-seg-${index}`;
    const isLast = index === trace.segments.length - 1;
    const segmentActive = isLiveSegmentActive(
      segment,
      isLast,
      trace.isStreaming,
      subagentById,
    );

    if (segment.kind === "thinking") {
      if (!segment.text.trim()) {
        return;
      }
      pendingItems.push({
        key,
        kind: "thinking",
        text: segment.text,
        isStreaming: trace.isStreaming && isLast,
        isActive: segmentActive,
        startedAtMs: segment.startedAtMs,
        durationMs: segment.durationMs,
      });
      pendingIsActive = pendingIsActive || segmentActive;
      return;
    }

    if (segment.kind === "execution") {
      const tools = segment.tools.filter(isVisibleTraceTool);
      const resolvedSubagents: ExecutionSubagent[] = segment.subagentRunIds
        .map((id) => subagentById.get(id))
        .filter((sub): sub is LiveSubagentRun => sub !== undefined)
        .map((sub) => ({
          kind: "live",
          key: sub.subagentRunId,
          subagent: sub,
        }));
      if (tools.length === 0 && resolvedSubagents.length === 0) {
        return;
      }
      pendingItems.push({
        key,
        kind: "execution",
        tools,
        subagents: resolvedSubagents,
      });
      pendingIsActive = pendingIsActive || segmentActive;
      return;
    }

    if (!segment.text.trim()) {
      return;
    }
    flushActivity();
    result.push({
      key,
      kind: "text",
      text: segment.text,
      isStreaming: trace.isStreaming && isLast,
    });
  });

  flushActivity();
  return result;
}

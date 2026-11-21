import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
import type {
  RunTraceSegment,
  RunTraceSubagent,
  RunTraceSummary,
  RunTraceToolItem,
} from "@/contracts/session-protocol";

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

function serverTimeMs(value: string | null | undefined): number | undefined {
  if (!value) {
    return undefined;
  }
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : undefined;
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

function liveSegmentFromTrace(
  segment: RunTraceSegment,
  index: number,
  lastIndex: number,
  isStreaming: boolean,
  fallbackStartedAtMs: number | undefined,
): LiveTraceSegment | null {
  if (segment.kind === "thinking") {
    return {
      kind: "thinking",
      text: segment.text,
      durationMs: segment.duration_ms ?? undefined,
      startedAtMs:
        serverTimeMs(segment.started_at) ??
        (isStreaming && index === lastIndex ? fallbackStartedAtMs : undefined),
    };
  }
  if (segment.kind === "execution") {
    const tools = toToolTraceItems(segment.tools);
    const subagentRunIds = (segment.subagents ?? []).map(
      (subagent) => subagent.subagent_run_id,
    );
    if (tools.length === 0 && subagentRunIds.length === 0) {
      return null;
    }
    return { kind: "execution", tools, subagentRunIds };
  }
  if (segment.kind === "user_message") {
    return {
      kind: "user_message",
      messageId: segment.message_id,
      content: segment.content,
      createdAt: segment.created_at,
    };
  }
  return { kind: "text", text: segment.text };
}

/**
 * Rebuild a live main-agent trace from the server's active-run snapshot.
 * `started_at` preserves the original `turn_started` clock across refreshes.
 */
function liveSegmentsFromTraceSegments(
  source: RunTraceSegment[],
  isStreaming: boolean,
  fallbackStartedAtMs: number | undefined,
): LiveTraceSegment[] {
  const lastIndex = source.length - 1;
  const segments: LiveTraceSegment[] = [];
  source.forEach((segment, index) => {
    const live = liveSegmentFromTrace(
      segment,
      index,
      lastIndex,
      isStreaming,
      fallbackStartedAtMs,
    );
    if (live) {
      segments.push(live);
    }
  });
  return segments;
}

export function liveTraceFromSummary(
  trace: RunTraceSummary,
  isStreaming = trace.status === "running",
): LiveAgentTrace {
  const startedAtMs = serverTimeMs(trace.started_at);
  const segments = liveSegmentsFromTraceSegments(
    trace.segments ?? [],
    isStreaming,
    startedAtMs,
  );
  return {
    agentId: "main",
    runId: trace.run_id,
    startedAtMs,
    segments,
    taskFocus: trace.task_focus
      ? {
          title: trace.task_focus.title,
          detail: trace.task_focus.detail ?? undefined,
          sectionId: trace.task_focus.section_id ?? undefined,
        }
      : undefined,
    isStreaming: isStreaming && trace.status === "running",
  };
}

function liveSubagentStatus(
  status: string,
): "running" | "completed" | "failed" {
  if (status === "running") {
    return "running";
  }
  if (status === "failed" || status === "error") {
    return "failed";
  }
  return "completed";
}

/**
 * Rebuild one subagent card from the active-run snapshot, including its
 * thinking clock, streamed tools, streamed text and task focus.
 */
export function liveSubagentFromSummary(
  subagent: RunTraceSubagent,
  isStreaming = subagent.status === "running",
): LiveSubagentRun {
  const status = liveSubagentStatus(subagent.status);
  const startedAtMs = serverTimeMs(subagent.started_at);
  const segments = liveSegmentsFromTraceSegments(
    subagent.segments ?? [],
    isStreaming,
    startedAtMs,
  );
  return {
    subagentRunId: subagent.subagent_run_id,
    agentName: subagent.agent_name,
    displayName: subagent.display_name ?? undefined,
    task: subagent.task,
    sectionId: subagent.section_id ?? undefined,
    status,
    summary: subagent.summary ?? undefined,
    trace: {
      agentId: subagent.agent_name,
      runId: subagent.subagent_run_id,
      startedAtMs,
      segments,
      taskFocus: subagent.task_focus
        ? {
            title: subagent.task_focus.title,
            detail: subagent.task_focus.detail ?? undefined,
            sectionId: subagent.task_focus.section_id ?? undefined,
          }
        : undefined,
      isStreaming: isStreaming && status === "running",
    },
  };
}

export interface HydratedActiveRun {
  main: LiveAgentTrace;
  subagents: LiveSubagentRun[];
}

/**
 * Hydrate the whole live run from the atomic `SessionDetail` snapshot. The
 * main trace already embeds subagent records; the dedicated array is preferred
 * when present because it is the flattened contract for active subagents.
 */
export function hydrateActiveRun(
  trace: RunTraceSummary,
  subagents: RunTraceSubagent[] | undefined,
  isStreaming = trace.status === "running",
): HydratedActiveRun {
  const main = liveTraceFromSummary(trace, isStreaming);
  const embedded: RunTraceSubagent[] = [];
  for (const segment of trace.segments ?? []) {
    if (segment.kind === "execution") {
      embedded.push(...(segment.subagents ?? []));
    }
  }
  const source = subagents && subagents.length > 0 ? subagents : embedded;
  const seen = new Set<string>();
  const liveSubagents: LiveSubagentRun[] = [];
  for (const subagent of source) {
    if (seen.has(subagent.subagent_run_id)) {
      continue;
    }
    seen.add(subagent.subagent_run_id);
    liveSubagents.push(liveSubagentFromSummary(subagent, isStreaming));
  }
  return { main, subagents: liveSubagents };
}

/**
 * Merge a server snapshot into locally streamed subagent cards. A running card
 * must accept a terminal server snapshot, but a completed card is never
 * regressed to an older running snapshot.
 */
export function mergeHydratedSubagents(
  previous: LiveSubagentRun[],
  hydrated: LiveSubagentRun[],
): LiveSubagentRun[] {
  const byId = new Map(
    previous.map((subagent) => [subagent.subagentRunId, subagent]),
  );
  for (const subagent of hydrated) {
    const existing = byId.get(subagent.subagentRunId);
    if (
      !existing ||
      (existing.status === "running" && subagent.status !== "running")
    ) {
      byId.set(subagent.subagentRunId, subagent);
    }
  }
  return Array.from(byId.values());
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
    taskFocus: undefined,
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
  if (
    trace.segments.some(
      (segment) =>
        segment.kind === "execution" &&
        segment.subagentRunIds.includes(subagentRunId),
    )
  ) {
    return trace;
  }
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
  truncated?: boolean;
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
            truncated: patch.truncated ?? tool.truncated,
          }
        : tool,
    );
    const segments = [...trace.segments];
    segments[index] = { ...segment, tools };
    return { ...trace, segments };
  }
  return trace;
}

import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
import { getToolMeta, isVisibleTraceTool } from "@/components/chat/toolMeta";
import type {
  RunTraceSegment,
  RunTraceSubagent,
  RunTraceSummary,
} from "@/contracts/session-protocol";
import {
  toToolTraceItems,
  type LiveAgentTrace,
  type LiveSubagentRun,
  type LiveTraceSegment,
} from "@/lib/liveTrace";

export interface SemanticActionPill {
  key: string;
  label: string;
  count: number;
  targets: string[];
  status: "running" | "completed" | "failed";
}

export interface MilestoneSubagentCard {
  id: string;
  actor: string;
  task: string;
  focusTitle?: string;
  focusDetail?: string;
  sectionId?: string;
  status: "running" | "completed" | "failed";
  semanticPills: SemanticActionPill[];
  milestones: TaskMilestoneNode[];
  summaryText?: string;
  toolCount: number;
  completedToolCount: number;
  rawTools: ToolCallTraceItem[];
  rawThinking: string;
}

export interface TaskMilestoneNode {
  id: string;
  stepNumber: number;
  title: string;
  detail?: string;
  sectionId?: string;
  status: "running" | "completed" | "failed";
  semanticPills: SemanticActionPill[];
  subagents: MilestoneSubagentCard[];
  rawTools: ToolCallTraceItem[];
  rawThinking: string;
  durationMs?: number;
}

function extractJsonStringField(raw: string, field: string): string | undefined {
  if (!raw) return undefined;
  const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`"${escaped}"\\s*:\\s*"([^"\\\\]{1,120})"`);
  const m = re.exec(raw);
  const val = m?.[1]?.trim();
  return val && val.length > 0 ? val : undefined;
}

export function parseDescribeTaskInput(inputPreview: string): {
  title?: string;
  detail?: string;
  sectionId?: string;
} {
  return {
    title: extractJsonStringField(inputPreview, "title"),
    detail: extractJsonStringField(inputPreview, "detail"),
    sectionId: extractJsonStringField(inputPreview, "section_id"),
  };
}

export function extractToolTargetHint(
  tool: ToolCallTraceItem,
): string | undefined {
  const raw = tool.inputPreview || "";
  if (!raw) return undefined;
  const section =
    extractJsonStringField(raw, "section_id") ??
    extractJsonStringField(raw, "block_id") ??
    extractJsonStringField(raw, "target_id");
  if (section) {
    return `#${section}`;
  }
  return (
    extractJsonStringField(raw, "keyword") ??
    extractJsonStringField(raw, "title") ??
    extractJsonStringField(raw, "query") ??
    extractJsonStringField(raw, "name") ??
    extractJsonStringField(raw, "path")
  );
}

/**
 * Group low-level tool calls inside a milestone into high-signal semantic
 * pills (e.g. "登记数据来源 ×3", "写入报告内容 · #c2-1") so the user sees
 * editorial actions rather than a wall of raw tool calls.
 */
export function buildSemanticActionPills(
  tools: ToolCallTraceItem[],
): SemanticActionPill[] {
  const visible = tools.filter(isVisibleTraceTool);
  const groups = new Map<
    string,
    {
      label: string;
      count: number;
      targets: string[];
      status: "running" | "completed" | "failed";
    }
  >();

  for (const tool of visible) {
    const meta = getToolMeta(tool.toolName);
    const existing = groups.get(meta.label);
    const target = extractToolTargetHint(tool);
    const toolStatus: "running" | "completed" | "failed" =
      tool.status === "running"
        ? "running"
        : tool.status === "failed"
          ? "failed"
          : "completed";

    if (!existing) {
      groups.set(meta.label, {
        label: meta.label,
        count: 1,
        targets: target ? [target] : [],
        status: toolStatus,
      });
    } else {
      existing.count += 1;
      if (
        target &&
        !existing.targets.includes(target) &&
        existing.targets.length < 2
      ) {
        existing.targets.push(target);
      }
      if (toolStatus === "running") {
        existing.status = "running";
      } else if (toolStatus === "failed" && existing.status !== "running") {
        existing.status = "failed";
      }
    }
  }

  return Array.from(groups.entries()).map(([key, value]) => ({
    key,
    ...value,
  }));
}

function defaultPhaseTitle(
  tools: ToolCallTraceItem[],
  hasSubagents: boolean,
  hasThinking: boolean,
  isFirst: boolean,
): string {
  if (hasSubagents && tools.length === 0) {
    return "派发子代理集群协同作业";
  }
  const firstVisible = tools.find(isVisibleTraceTool);
  if (firstVisible) {
    return getToolMeta(firstVisible.toolName).label;
  }
  if (hasThinking) {
    return isFirst ? "需求拆解与研究规划" : "阶段研判与统筹";
  }
  return "任务执行";
}

function buildSubagentMilestonesFromSegments(
  subId: string,
  segments: RunTraceSegment[],
  fallbackFocus: { title?: string; detail?: string; sectionId?: string },
  subStatus: "running" | "completed" | "failed",
): TaskMilestoneNode[] {
  const milestones: TaskMilestoneNode[] = [];
  let currentTitle = fallbackFocus.title;
  let currentDetail = fallbackFocus.detail;
  let currentSection = fallbackFocus.sectionId;
  let currentTools: ToolCallTraceItem[] = [];
  let currentThinking: string[] = [];
  let currentDuration = 0;

  const flush = (isFinal: boolean) => {
    const visibleTools = currentTools.filter(isVisibleTraceTool);
    if (
      !currentTitle &&
      visibleTools.length === 0 &&
      currentThinking.length === 0
    ) {
      return;
    }
    const stepNumber = milestones.length + 1;
    const hasRunning = visibleTools.some((t) => t.status === "running");
    const hasFailed = visibleTools.some((t) => t.status === "failed");
    const status: "running" | "completed" | "failed" =
      hasRunning || (isFinal && subStatus === "running")
        ? "running"
        : hasFailed || (isFinal && subStatus === "failed")
          ? "failed"
          : "completed";

    milestones.push({
      id: `${subId}-ms-${stepNumber}`,
      stepNumber,
      title:
        currentTitle ||
        defaultPhaseTitle(
          visibleTools,
          false,
          currentThinking.length > 0,
          stepNumber === 1,
        ),
      detail: currentDetail,
      sectionId: currentSection,
      status,
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: [],
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });
    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentTools = [];
    currentThinking = [];
    currentDuration = 0;
  };

  for (const seg of segments) {
    if (seg.kind === "thinking") {
      if (seg.text.trim()) {
        currentThinking.push(seg.text.trim());
      }
      if (typeof seg.duration_ms === "number") {
        currentDuration += seg.duration_ms;
      }
      continue;
    }
    if (seg.kind === "execution") {
      const items = toToolTraceItems(seg.tools);
      for (const tool of items) {
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            if (currentTitle || currentTools.some(isVisibleTraceTool)) {
              flush(false);
            }
            currentTitle = parsed.title;
            currentDetail = parsed.detail;
            currentSection = parsed.sectionId;
          }
          continue;
        }
        currentTools.push(tool);
      }
    }
  }

  flush(true);
  return milestones;
}

function buildLiveSubagentMilestonesFromSegments(
  subId: string,
  segments: LiveTraceSegment[],
  fallbackFocus: { title?: string; detail?: string; sectionId?: string },
  subStatus: "running" | "completed" | "failed",
): TaskMilestoneNode[] {
  const milestones: TaskMilestoneNode[] = [];
  let currentTitle = fallbackFocus.title;
  let currentDetail = fallbackFocus.detail;
  let currentSection = fallbackFocus.sectionId;
  let currentTools: ToolCallTraceItem[] = [];
  let currentThinking: string[] = [];
  let currentDuration = 0;

  const flush = (isFinal: boolean) => {
    const visibleTools = currentTools.filter(isVisibleTraceTool);
    if (
      !currentTitle &&
      visibleTools.length === 0 &&
      currentThinking.length === 0
    ) {
      return;
    }
    const stepNumber = milestones.length + 1;
    const hasRunning = visibleTools.some((t) => t.status === "running");
    const hasFailed = visibleTools.some((t) => t.status === "failed");
    const status: "running" | "completed" | "failed" =
      hasRunning || (isFinal && subStatus === "running")
        ? "running"
        : hasFailed || (isFinal && subStatus === "failed")
          ? "failed"
          : "completed";

    milestones.push({
      id: `${subId}-ms-${stepNumber}`,
      stepNumber,
      title:
        currentTitle ||
        defaultPhaseTitle(
          visibleTools,
          false,
          currentThinking.length > 0,
          stepNumber === 1,
        ),
      detail: currentDetail,
      sectionId: currentSection,
      status,
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: [],
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });
    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentTools = [];
    currentThinking = [];
    currentDuration = 0;
  };

  for (const seg of segments) {
    if (seg.kind === "thinking") {
      if (seg.text.trim()) {
        currentThinking.push(seg.text.trim());
      }
      if (typeof seg.durationMs === "number") {
        currentDuration += seg.durationMs;
      }
      continue;
    }
    if (seg.kind === "execution") {
      for (const tool of seg.tools) {
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            if (currentTitle || currentTools.some(isVisibleTraceTool)) {
              flush(false);
            }
            currentTitle = parsed.title;
            currentDetail = parsed.detail;
            currentSection = parsed.sectionId;
          }
          continue;
        }
        currentTools.push(tool);
      }
    }
  }

  flush(true);
  return milestones;
}

function buildHistoricalSubagentCard(
  sub: RunTraceSubagent,
): MilestoneSubagentCard {
  const status: "running" | "completed" | "failed" =
    sub.status === "failed"
      ? "failed"
      : sub.status === "running"
        ? "running"
        : "completed";
  const allTools: ToolCallTraceItem[] = [];
  const thinkingParts: string[] = [];
  const textParts: string[] = [];
  let lastDescribeTitle: string | undefined;
  let lastDescribeDetail: string | undefined;
  let lastDescribeSection: string | undefined;

  for (const seg of sub.segments ?? []) {
    if (seg.kind === "thinking" && seg.text.trim()) {
      thinkingParts.push(seg.text.trim());
    } else if (seg.kind === "text" && seg.text.trim()) {
      textParts.push(seg.text.trim());
    } else if (seg.kind === "execution") {
      for (const tool of toToolTraceItems(seg.tools)) {
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            lastDescribeTitle = parsed.title;
            lastDescribeDetail = parsed.detail;
            lastDescribeSection = parsed.sectionId;
          }
        } else if (isVisibleTraceTool(tool)) {
          allTools.push(tool);
        }
      }
    }
  }

  const focusTitle =
    sub.task_focus?.title?.trim() || lastDescribeTitle || undefined;
  const focusDetail =
    sub.task_focus?.detail?.trim() || lastDescribeDetail || undefined;
  const sectionId =
    sub.section_id?.trim() ||
    sub.task_focus?.section_id?.trim() ||
    lastDescribeSection ||
    undefined;

  const milestones = buildSubagentMilestonesFromSegments(
    sub.subagent_run_id,
    sub.segments ?? [],
    { title: focusTitle, detail: focusDetail, sectionId },
    status,
  );

  return {
    id: sub.subagent_run_id,
    actor: sub.display_name?.trim() || sub.agent_name,
    task: sub.task,
    focusTitle,
    focusDetail,
    sectionId,
    status,
    semanticPills: buildSemanticActionPills(allTools),
    milestones,
    summaryText:
      sub.summary?.trim() || textParts.join("\n\n").trim() || undefined,
    toolCount: allTools.length,
    completedToolCount: allTools.filter((t) => t.status !== "running").length,
    rawTools: allTools,
    rawThinking: thinkingParts.join("\n\n").trim(),
  };
}

function buildLiveSubagentCard(sub: LiveSubagentRun): MilestoneSubagentCard {
  const allTools: ToolCallTraceItem[] = [];
  const thinkingParts: string[] = [];
  const textParts: string[] = [];
  let lastDescribeTitle: string | undefined;
  let lastDescribeDetail: string | undefined;
  let lastDescribeSection: string | undefined;

  for (const seg of sub.trace.segments) {
    if (seg.kind === "thinking" && seg.text.trim()) {
      thinkingParts.push(seg.text.trim());
    } else if (seg.kind === "text" && seg.text.trim()) {
      textParts.push(seg.text.trim());
    } else if (seg.kind === "execution") {
      for (const tool of seg.tools) {
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            lastDescribeTitle = parsed.title;
            lastDescribeDetail = parsed.detail;
            lastDescribeSection = parsed.sectionId;
          }
        } else if (isVisibleTraceTool(tool)) {
          allTools.push(tool);
        }
      }
    }
  }

  const focusTitle =
    sub.trace.taskFocus?.title?.trim() || lastDescribeTitle || undefined;
  const focusDetail =
    sub.trace.taskFocus?.detail?.trim() || lastDescribeDetail || undefined;
  const sectionId =
    sub.sectionId?.trim() ||
    sub.trace.taskFocus?.sectionId?.trim() ||
    lastDescribeSection ||
    undefined;

  const milestones = buildLiveSubagentMilestonesFromSegments(
    sub.subagentRunId,
    sub.trace.segments,
    { title: focusTitle, detail: focusDetail, sectionId },
    sub.status,
  );

  return {
    id: sub.subagentRunId,
    actor: sub.displayName?.trim() || sub.agentName,
    task: sub.task,
    focusTitle,
    focusDetail,
    sectionId,
    status: sub.status,
    semanticPills: buildSemanticActionPills(allTools),
    milestones,
    summaryText:
      sub.summary?.trim() || textParts.join("\n\n").trim() || undefined,
    toolCount: allTools.length,
    completedToolCount: allTools.filter((t) => t.status !== "running").length,
    rawTools: allTools,
    rawThinking: thinkingParts.join("\n\n").trim(),
  };
}

/**
 * Reconstruct the ordered `describe_task` milestone timeline from a historical
 * `RunTraceSummary`. Every `describe_task` tool call opens a new editorial
 * milestone and groups subsequent tools, thinking, and spawned subagents under it.
 */
export function buildHistoricalMilestones(
  trace: RunTraceSummary,
): TaskMilestoneNode[] {
  const milestones: TaskMilestoneNode[] = [];
  let currentTitle: string | undefined;
  let currentDetail: string | undefined;
  let currentSection: string | undefined;
  let currentTools: ToolCallTraceItem[] = [];
  let currentSubagents: MilestoneSubagentCard[] = [];
  let currentThinking: string[] = [];
  let currentDuration = 0;

  const flush = (isFinal: boolean) => {
    const visibleTools = currentTools.filter(isVisibleTraceTool);
    if (
      !currentTitle &&
      visibleTools.length === 0 &&
      currentSubagents.length === 0 &&
      currentThinking.length === 0
    ) {
      return;
    }
    const stepNumber = milestones.length + 1;
    const hasFailed =
      visibleTools.some((t) => t.status === "failed") ||
      currentSubagents.some((s) => s.status === "failed") ||
      (isFinal && trace.status === "failed");
    const status: "running" | "completed" | "failed" = hasFailed
      ? "failed"
      : "completed";

    milestones.push({
      id: `${trace.run_id}-ms-${stepNumber}`,
      stepNumber,
      title:
        currentTitle ||
        (isFinal && trace.task_focus?.title?.trim()) ||
        defaultPhaseTitle(
          visibleTools,
          currentSubagents.length > 0,
          currentThinking.length > 0,
          stepNumber === 1,
        ),
      detail:
        currentDetail ??
        (isFinal ? trace.task_focus?.detail ?? undefined : undefined),
      sectionId:
        currentSection ??
        (isFinal ? trace.task_focus?.section_id ?? undefined : undefined),
      status,
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: currentSubagents,
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });

    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentTools = [];
    currentSubagents = [];
    currentThinking = [];
    currentDuration = 0;
  };

  for (const seg of trace.segments ?? []) {
    if (seg.kind === "thinking") {
      if (seg.text.trim()) {
        currentThinking.push(seg.text.trim());
      }
      if (typeof seg.duration_ms === "number") {
        currentDuration += seg.duration_ms;
      }
      continue;
    }
    if (seg.kind === "execution") {
      for (const tool of toToolTraceItems(seg.tools)) {
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            if (
              currentTitle ||
              currentTools.some(isVisibleTraceTool) ||
              currentSubagents.length > 0
            ) {
              flush(false);
            }
            currentTitle = parsed.title;
            currentDetail = parsed.detail;
            currentSection = parsed.sectionId;
          }
          continue;
        }
        currentTools.push(tool);
      }
      for (const sub of seg.subagents ?? []) {
        currentSubagents.push(buildHistoricalSubagentCard(sub));
      }
    }
  }

  flush(true);

  if (milestones.length === 0 && trace.task_focus?.title) {
    milestones.push({
      id: `${trace.run_id}-ms-1`,
      stepNumber: 1,
      title: trace.task_focus.title,
      detail: trace.task_focus.detail ?? undefined,
      sectionId: trace.task_focus.section_id ?? undefined,
      status: trace.status === "failed" ? "failed" : "completed",
      semanticPills: [],
      subagents: [],
      rawTools: [],
      rawThinking: "",
      durationMs: trace.duration_ms ?? undefined,
    });
  }

  return milestones;
}

/**
 * Reconstruct the ordered `describe_task` milestone timeline from a live
 * streaming `LiveAgentTrace` and its active `LiveSubagentRun[]`.
 */
export function buildLiveMilestones(
  trace: LiveAgentTrace,
  subagents: LiveSubagentRun[],
): TaskMilestoneNode[] {
  const subById = new Map(subagents.map((s) => [s.subagentRunId, s]));
  const milestones: TaskMilestoneNode[] = [];
  let currentTitle: string | undefined;
  let currentDetail: string | undefined;
  let currentSection: string | undefined;
  let currentTools: ToolCallTraceItem[] = [];
  let currentSubagents: MilestoneSubagentCard[] = [];
  let currentThinking: string[] = [];
  let currentDuration = 0;

  const flush = (isFinal: boolean) => {
    const visibleTools = currentTools.filter(isVisibleTraceTool);
    if (
      !currentTitle &&
      visibleTools.length === 0 &&
      currentSubagents.length === 0 &&
      currentThinking.length === 0
    ) {
      return;
    }
    const stepNumber = milestones.length + 1;
    const hasRunning =
      (isFinal && trace.isStreaming) ||
      visibleTools.some((t) => t.status === "running") ||
      currentSubagents.some((s) => s.status === "running");
    const hasFailed =
      visibleTools.some((t) => t.status === "failed") ||
      currentSubagents.some((s) => s.status === "failed");

    const status: "running" | "completed" | "failed" = hasRunning
      ? "running"
      : hasFailed
        ? "failed"
        : "completed";

    milestones.push({
      id: `${trace.runId ?? "live"}-ms-${stepNumber}`,
      stepNumber,
      title:
        currentTitle ||
        (isFinal && trace.taskFocus?.title?.trim()) ||
        defaultPhaseTitle(
          visibleTools,
          currentSubagents.length > 0,
          currentThinking.length > 0,
          stepNumber === 1,
        ),
      detail:
        currentDetail ?? (isFinal ? trace.taskFocus?.detail : undefined),
      sectionId:
        currentSection ?? (isFinal ? trace.taskFocus?.sectionId : undefined),
      status,
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: currentSubagents,
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });

    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentTools = [];
    currentSubagents = [];
    currentThinking = [];
    currentDuration = 0;
  };

  for (const seg of trace.segments) {
    if (seg.kind === "thinking") {
      if (seg.text.trim()) {
        currentThinking.push(seg.text.trim());
      }
      if (typeof seg.durationMs === "number") {
        currentDuration += seg.durationMs;
      }
      continue;
    }
    if (seg.kind === "execution") {
      for (const tool of seg.tools) {
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            if (
              currentTitle ||
              currentTools.some(isVisibleTraceTool) ||
              currentSubagents.length > 0
            ) {
              flush(false);
            }
            currentTitle = parsed.title;
            currentDetail = parsed.detail;
            currentSection = parsed.sectionId;
          }
          continue;
        }
        currentTools.push(tool);
      }
      for (const subId of seg.subagentRunIds) {
        const sub = subById.get(subId);
        if (sub) {
          currentSubagents.push(buildLiveSubagentCard(sub));
        }
      }
    }
  }

  flush(true);

  if (milestones.length === 0 && trace.taskFocus?.title) {
    milestones.push({
      id: `${trace.runId ?? "live"}-ms-1`,
      stepNumber: 1,
      title: trace.taskFocus.title,
      detail: trace.taskFocus.detail,
      sectionId: trace.taskFocus.sectionId,
      status: trace.isStreaming ? "running" : "completed",
      semanticPills: [],
      subagents: [],
      rawTools: [],
      rawThinking: "",
    });
  }

  return milestones;
}

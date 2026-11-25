import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
import { getToolMeta, isVisibleTraceTool } from "@/components/chat/toolMeta";
import type {
  RunTraceSegment,
  RunTraceSubagent,
  RunTraceSummary,
  TaskTodoItem,
  TaskTodoStatus,
} from "@/contracts/session-protocol";
import { isRecord } from "@/lib/guards";
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
  todos: TaskTodoItem[];
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
  isExplicitStage?: boolean;
  semanticPills: SemanticActionPill[];
  subagents: MilestoneSubagentCard[];
  rawTools: ToolCallTraceItem[];
  rawThinking: string;
  durationMs?: number;
}

/**
 * Low-level protocol and internal reading plumbing tools that Qingta service
 * analysts and higher-ed faculty do not need to see as semantic action pills
 * unless a stage has no domain-level tools at all.
 */
const LOW_SIGNAL_PLUMBING_TOOLS = new Set<string>([
  "get_protocol_guide",
  "list_skills",
  "load_skill",
  "read_skill_resource",
  "list_subagents",
  "get_document_digest",
  "read_blocks",
  "read_as_markdown",
  "list_reports",
  "list_sources",
  "list_comments",
  "list_attachments",
]);

export function isDomainMeaningfulTool(tool: ToolCallTraceItem): boolean {
  if (!isVisibleTraceTool(tool)) {
    return false;
  }
  if (tool.status === "failed") {
    return true;
  }
  return !LOW_SIGNAL_PLUMBING_TOOLS.has(tool.toolName);
}

function extractJsonStringField(raw: string, field: string): string | undefined {
  if (!raw) return undefined;
  const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`"${escaped}"\\s*:\\s*"([^"\\\\]{1,120})"`);
  const m = re.exec(raw);
  const val = m?.[1]?.trim();
  return val && val.length > 0 ? val : undefined;
}

function normalizeTodoStatus(raw: unknown): TaskTodoStatus {
  if (
    raw === "pending" ||
    raw === "in_progress" ||
    raw === "completed" ||
    raw === "skipped"
  ) {
    return raw;
  }
  return "pending";
}

function parseTodosFromParsedJson(rawTodos: unknown): TaskTodoItem[] {
  if (!Array.isArray(rawTodos)) return [];
  const items: TaskTodoItem[] = [];
  for (const entry of rawTodos) {
    if (!isRecord(entry) || typeof entry.title !== "string") continue;
    const title = entry.title.trim();
    if (!title) continue;
    items.push({
      id: typeof entry.id === "string" ? entry.id : undefined,
      title,
      status: normalizeTodoStatus(entry.status),
      detail: typeof entry.detail === "string" ? entry.detail : undefined,
      section_id:
        typeof entry.section_id === "string" ? entry.section_id : undefined,
    });
  }
  return items;
}

export function parseDescribeTaskInput(inputPreview: string): {
  title?: string;
  detail?: string;
  sectionId?: string;
} {
  if (!inputPreview) return {};
  try {
    const parsed: unknown = JSON.parse(inputPreview);
    if (isRecord(parsed)) {
      return {
        title:
          typeof parsed.title === "string" && parsed.title.trim()
            ? parsed.title.trim()
            : undefined,
        detail:
          typeof parsed.detail === "string" && parsed.detail.trim()
            ? parsed.detail.trim()
            : undefined,
        sectionId:
          typeof parsed.section_id === "string" && parsed.section_id.trim()
            ? parsed.section_id.trim()
            : undefined,
      };
    }
  } catch {
    // Fallback to regex extraction when JSON is partial.
  }
  return {
    title: extractJsonStringField(inputPreview, "title"),
    detail: extractJsonStringField(inputPreview, "detail"),
    sectionId: extractJsonStringField(inputPreview, "section_id"),
  };
}

export function parseUpdateTodosInput(inputPreview: string): TaskTodoItem[] {
  if (!inputPreview) return [];
  try {
    const parsed: unknown = JSON.parse(inputPreview);
    if (isRecord(parsed)) {
      return parseTodosFromParsedJson(parsed.todos);
    }
  } catch {
    // Ignore truncated preview payloads; complete todos arrive on `todos_updated` events.
  }
  return [];
}

function collectTodosFromTools(
  initial: TaskTodoItem[] | null | undefined,
  tools: Iterable<{ toolName: string; inputPreview: string }>,
): TaskTodoItem[] {
  if (initial && initial.length > 0) {
    return initial.map((item) => ({ ...item }));
  }
  let todos: TaskTodoItem[] = [];
  for (const tool of tools) {
    if (tool.toolName === "update_todos") {
      const parsed = parseUpdateTodosInput(tool.inputPreview);
      if (parsed.length > 0) {
        todos = parsed;
      }
    }
  }
  return todos;
}

export function extractHistoricalSubagentTodos(
  sub: RunTraceSubagent,
): TaskTodoItem[] {
  const segmentTools: Array<{ toolName: string; inputPreview: string }> = [];
  for (const seg of sub.segments ?? []) {
    if (seg.kind === "execution") {
      for (const tool of toToolTraceItems(seg.tools)) {
        segmentTools.push({
          toolName: tool.toolName,
          inputPreview: tool.inputPreview,
        });
      }
    }
  }
  const todos = collectTodosFromTools(sub.task_focus?.todos, segmentTools);
  return finalizeTodos(todos, sub.status !== "failed" && sub.status !== "running");
}

export function finalizeTodos(
  todos: TaskTodoItem[],
  isSettledSuccess: boolean,
): TaskTodoItem[] {
  if (!isSettledSuccess || todos.length === 0) {
    return todos;
  }
  return todos.map((item) =>
    item.status === "skipped"
      ? item
      : { ...item, status: "completed" as const },
  );
}

/**
 * Extract a human-readable domain target (such as a university name, search
 * keyword, or report title) without leaking internal `#block_id` strings or
 * sandbox file paths to end users.
 */
export function extractToolTargetHint(
  tool: ToolCallTraceItem,
): string | undefined {
  const raw = tool.inputPreview || "";
  if (!raw) return undefined;
  const candidate =
    extractJsonStringField(raw, "university") ??
    extractJsonStringField(raw, "school_name") ??
    extractJsonStringField(raw, "keyword") ??
    extractJsonStringField(raw, "query") ??
    extractJsonStringField(raw, "title") ??
    extractJsonStringField(raw, "subject_name");
  if (!candidate) {
    return undefined;
  }
  // Never surface raw block IDs (#c2-1) or internal file paths
  if (
    candidate.startsWith("#") ||
    candidate.startsWith("/") ||
    /^[a-z0-9_-]+$/i.test(candidate)
  ) {
    return undefined;
  }
  return candidate;
}

/**
 * Group tool calls inside a milestone into clean, user-meaningful semantic
 * action tags (e.g. "查询高教全景数据 · 浙江大学", "登记数据来源", "写入报告内容")
 * while filtering out internal protocol plumbing (`get_protocol_guide`,
 * `load_skill`, `read_blocks`) when domain actions exist.
 */
export function buildSemanticActionPills(
  tools: ToolCallTraceItem[],
): SemanticActionPill[] {
  const visible = tools.filter(isVisibleTraceTool);
  const meaningful = visible.filter(isDomainMeaningfulTool);
  const sourceTools = meaningful.length > 0 ? meaningful : visible;

  const groups = new Map<
    string,
    {
      label: string;
      count: number;
      targets: string[];
      status: "running" | "completed" | "failed";
    }
  >();

  for (const tool of sourceTools) {
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
  const visible = tools.filter(isVisibleTraceTool);
  const meaningful = visible.filter(isDomainMeaningfulTool);
  const pool = meaningful.length > 0 ? meaningful : visible;

  if (hasSubagents && pool.length === 0) {
    return "多角色协同调研与撰写";
  }
  if (pool.length > 0) {
    const distinctLabels: string[] = [];
    for (const tool of pool) {
      const label = getToolMeta(tool.toolName).label;
      if (!distinctLabels.includes(label)) {
        distinctLabels.push(label);
      }
      if (distinctLabels.length >= 2) break;
    }
    return distinctLabels.join(" · ");
  }
  if (hasThinking) {
    return isFirst ? "需求拆解与研究规划" : "阶段研判与统筹";
  }
  return "处理研究任务";
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
  let currentExplicit = Boolean(fallbackFocus.title);
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
      isExplicitStage: currentExplicit,
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: [],
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });
    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentExplicit = false;
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
        if (tool.toolName === "update_todos") {
          continue;
        }
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            if (currentTitle || currentTools.some(isVisibleTraceTool)) {
              flush(false);
            }
            currentTitle = parsed.title;
            currentDetail = parsed.detail;
            currentSection = parsed.sectionId;
            currentExplicit = true;
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
  let currentExplicit = Boolean(fallbackFocus.title);
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
      isExplicitStage: currentExplicit,
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: [],
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });
    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentExplicit = false;
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
        if (tool.toolName === "update_todos") {
          continue;
        }
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            if (currentTitle || currentTools.some(isVisibleTraceTool)) {
              flush(false);
            }
            currentTitle = parsed.title;
            currentDetail = parsed.detail;
            currentSection = parsed.sectionId;
            currentExplicit = true;
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
  const todoTools: Array<{ toolName: string; inputPreview: string }> = [];

  for (const seg of sub.segments ?? []) {
    if (seg.kind === "thinking" && seg.text.trim()) {
      thinkingParts.push(seg.text.trim());
    } else if (seg.kind === "text" && seg.text.trim()) {
      textParts.push(seg.text.trim());
    } else if (seg.kind === "execution") {
      for (const tool of toToolTraceItems(seg.tools)) {
        if (tool.toolName === "update_todos") {
          todoTools.push({
            toolName: tool.toolName,
            inputPreview: tool.inputPreview,
          });
          continue;
        }
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            lastDescribeTitle = parsed.title;
            lastDescribeDetail = parsed.detail;
            lastDescribeSection = parsed.sectionId;
          }
          todoTools.push({
            toolName: tool.toolName,
            inputPreview: tool.inputPreview,
          });
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

  const todos = collectTodosFromTools(sub.task_focus?.todos, todoTools);
  return {
    id: sub.subagent_run_id,
    actor: sub.display_name?.trim() || sub.agent_name,
    task: sub.task,
    focusTitle,
    focusDetail,
    sectionId,
    status,
    todos: finalizeTodos(todos, status === "completed"),
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
  const todoTools: Array<{ toolName: string; inputPreview: string }> = [];

  for (const seg of sub.trace.segments) {
    if (seg.kind === "thinking" && seg.text.trim()) {
      thinkingParts.push(seg.text.trim());
    } else if (seg.kind === "text" && seg.text.trim()) {
      textParts.push(seg.text.trim());
    } else if (seg.kind === "execution") {
      for (const tool of seg.tools) {
        if (tool.toolName === "update_todos") {
          todoTools.push({
            toolName: tool.toolName,
            inputPreview: tool.inputPreview,
          });
          continue;
        }
        if (tool.toolName === "describe_task") {
          const parsed = parseDescribeTaskInput(tool.inputPreview);
          if (parsed.title) {
            lastDescribeTitle = parsed.title;
            lastDescribeDetail = parsed.detail;
            lastDescribeSection = parsed.sectionId;
          }
          todoTools.push({
            toolName: tool.toolName,
            inputPreview: tool.inputPreview,
          });
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
  const todos = collectTodosFromTools(sub.trace.taskFocus?.todos, todoTools);

  return {
    id: sub.subagentRunId,
    actor: sub.displayName?.trim() || sub.agentName,
    task: sub.task,
    focusTitle,
    focusDetail,
    sectionId,
    status: sub.status,
    todos: finalizeTodos(todos, sub.status === "completed"),
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

export function extractHistoricalTodos(trace: RunTraceSummary): TaskTodoItem[] {
  const segmentTools: Array<{ toolName: string; inputPreview: string }> = [];
  for (const seg of trace.segments ?? []) {
    if (seg.kind !== "execution") continue;
    for (const tool of seg.tools ?? []) {
      segmentTools.push({
        toolName: tool.tool_name,
        inputPreview: tool.input_preview,
      });
    }
  }
  const todos = collectTodosFromTools(trace.task_focus?.todos, segmentTools);
  return finalizeTodos(todos, trace.status === "completed");
}

export function extractLiveTodos(trace: LiveAgentTrace): TaskTodoItem[] {
  const segmentTools: Array<{ toolName: string; inputPreview: string }> = [];
  for (const seg of trace.segments) {
    if (seg.kind !== "execution") continue;
    for (const tool of seg.tools) {
      segmentTools.push({
        toolName: tool.toolName,
        inputPreview: tool.inputPreview,
      });
    }
  }
  const todos = collectTodosFromTools(trace.taskFocus?.todos, segmentTools);
  return finalizeTodos(todos, !trace.isStreaming);
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
  let currentExplicit = false;
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
    const explicitTitle =
      currentTitle || (isFinal ? trace.task_focus?.title?.trim() : undefined);

    milestones.push({
      id: `${trace.run_id}-ms-${stepNumber}`,
      stepNumber,
      title:
        explicitTitle ||
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
      isExplicitStage: currentExplicit || Boolean(explicitTitle),
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: currentSubagents,
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });

    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentExplicit = false;
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
        if (tool.toolName === "update_todos") {
          continue;
        }
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
            currentExplicit = true;
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
      isExplicitStage: true,
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
  let currentExplicit = false;
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
    const explicitTitle =
      currentTitle || (isFinal ? trace.taskFocus?.title?.trim() : undefined);

    milestones.push({
      id: `${trace.runId ?? "live"}-ms-${stepNumber}`,
      stepNumber,
      title:
        explicitTitle ||
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
      isExplicitStage: currentExplicit || Boolean(explicitTitle),
      semanticPills: buildSemanticActionPills(visibleTools),
      subagents: currentSubagents,
      rawTools: visibleTools,
      rawThinking: currentThinking.join("\n\n").trim(),
      durationMs: currentDuration > 0 ? currentDuration : undefined,
    });

    currentTitle = undefined;
    currentDetail = undefined;
    currentSection = undefined;
    currentExplicit = false;
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
        if (tool.toolName === "update_todos") {
          continue;
        }
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
            currentExplicit = true;
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
      isExplicitStage: true,
      semanticPills: [],
      subagents: [],
      rawTools: [],
      rawThinking: "",
    });
  }

  return milestones;
}

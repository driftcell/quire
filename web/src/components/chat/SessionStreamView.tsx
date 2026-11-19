import {
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  CompassIcon,
  CopyIcon,
  FileTextIcon,
  ListTreeIcon,
  LoaderCircleIcon,
  SparklesIcon,
  SquareIcon,
  UsersIcon,
} from "lucide-react";
import { Streamdown, type ControlsConfig } from "streamdown";
import { QuestionCard } from "@/components/chat/QuestionCard";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import {
  ToolCallTrace,
  type ToolCallTraceItem,
} from "@/components/chat/ToolCallTrace";
import {
  Attachment,
  AttachmentContent,
  AttachmentDescription,
  AttachmentGroup,
  AttachmentMedia,
  AttachmentTitle,
} from "@/components/ui/attachment";
import { Button } from "@/components/ui/button";
import { Message, MessageContent, MessageGroup } from "@/components/ui/message";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
  useMessageScroller,
} from "@/components/ui/message-scroller";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";
import type {
  AttachmentMeta,
  PendingQuestion,
  QuestionAnswer,
  RunTraceSubagent,
  RunTraceSummary,
  RunTraceToolItem,
  SessionMessage,
} from "@/contracts/session-protocol";
import type {
  LiveAgentTrace,
  LiveSubagentRun,
  LiveTraceSegment,
} from "@/lib/liveTrace";

const CHAT_PROSE =
  "prose prose-sm dark:prose-invert max-w-none prose-headings:font-semibold prose-headings:mt-3 prose-headings:mb-1.5 prose-h1:text-base prose-h2:text-[15px] prose-h3:text-sm prose-p:my-1.5 prose-p:leading-6 prose-ul:my-1.5 prose-ol:my-1.5 prose-pre:my-2 prose-pre:text-xs prose-code:text-[12.5px]";

const STREAMDOWN_CONTROLS: ControlsConfig = {
  table: false,
  code: true,
};

export interface SessionStreamViewProps {
  sessionId: string;
  messages: SessionMessage[];
  optimisticMessages?: SessionMessage[];
  attachments?: AttachmentMeta[];
  traces?: RunTraceSummary[];
  liveMain: LiveAgentTrace | null;
  liveSubagents: LiveSubagentRun[];
  pendingQuestion: PendingQuestion | null;
  resolvedQuestions?: Array<{
    question: PendingQuestion;
    status: "answered" | "skipped" | "dismissed";
    answers?: QuestionAnswer[];
  }>;
  isAnsweringQuestion: boolean;
  onAnswerQuestion: (
    questionId: string,
    answers: QuestionAnswer[],
    skipped: boolean,
  ) => void;
  onJumpToSection?: (sectionId: string) => void;
  onInterrupt?: () => Promise<void>;
  contentClassName?: string;
}

const timeFormatter = new Intl.DateTimeFormat("zh-CN", {
  month: "numeric",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
});

const fullTimeFormatter = new Intl.DateTimeFormat("zh-CN", {
  dateStyle: "medium",
  timeStyle: "medium",
});

function formatMsgTime(iso: string): { label: string; full: string } | null {
  const date = new Date(iso);
  if (!Number.isFinite(date.getTime())) return null;
  return {
    label: timeFormatter.format(date),
    full: fullTimeFormatter.format(date),
  };
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

async function copyText(text: string): Promise<void> {
  const clipboard = navigator.clipboard;
  if (clipboard && typeof clipboard.writeText === "function") {
    await clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  try {
    document.execCommand("copy");
  } finally {
    textarea.remove();
  }
}

function CopyMessageButton({ content }: { content: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await copyText(content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={copied ? "已复制" : "拷贝回复"}
            onClick={() => {
              void handleCopy();
            }}
            className="text-muted-foreground hover:text-foreground"
          />
        }
      >
        {copied ? (
          <CheckIcon className="size-3.5 text-emerald-600 dark:text-emerald-400" />
        ) : (
          <CopyIcon className="size-3.5" />
        )}
      </TooltipTrigger>
      <TooltipContent>{copied ? "已复制" : "拷贝"}</TooltipContent>
    </Tooltip>
  );
}

const COLLAPSE_THRESHOLD = 5;
const VISIBLE_RECENT_TOOLS = 4;

const ToolCallList = memo(function ToolCallList({
  sessionId,
  tools: rawTools,
}: {
  sessionId: string;
  tools: ToolCallTraceItem[];
}) {
  const [expanded, setExpanded] = useState(false);
  const tools = useMemo(
    () => rawTools.filter((t) => t.toolName !== "describe_task"),
    [rawTools],
  );

  if (tools.length === 0) {
    return null;
  }

  if (tools.length <= COLLAPSE_THRESHOLD || expanded) {
    return (
      <div className="space-y-1">
        {tools.length > COLLAPSE_THRESHOLD && (
          <button
            type="button"
            onClick={() => setExpanded(false)}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-muted-foreground tabular-nums hover:bg-muted hover:text-foreground"
          >
            <ChevronUpIcon className="size-3" />
            <span>
              收起更早的 {tools.length - VISIBLE_RECENT_TOOLS} 个调用（共{" "}
              {tools.length} 个）
            </span>
          </button>
        )}
        {tools.map((tool) => (
          <ToolCallTrace
            key={tool.toolCallId}
            sessionId={sessionId}
            item={tool}
          />
        ))}
      </div>
    );
  }

  const splitAt = tools.length - VISIBLE_RECENT_TOOLS;
  const hiddenTools = tools.slice(0, splitAt);
  const recentTools = tools.slice(splitAt);
  const runningHidden = hiddenTools.filter((t) => t.status === "running");

  return (
    <div className="space-y-1">
      <button
        type="button"
        onClick={() => setExpanded(true)}
        className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-muted-foreground tabular-nums hover:bg-muted hover:text-foreground"
      >
        <ChevronDownIcon className="size-3" />
        <span>
          显示更早的 {hiddenTools.length} 个调用（共 {tools.length} 个）
        </span>
      </button>
      {runningHidden.map((tool) => (
        <ToolCallTrace
          key={tool.toolCallId}
          sessionId={sessionId}
          item={tool}
        />
      ))}
      {recentTools.map((tool) => (
        <ToolCallTrace
          key={tool.toolCallId}
          sessionId={sessionId}
          item={tool}
        />
      ))}
    </div>
  );
});

function traceToolStatus(status: string): ToolCallTraceItem["status"] {
  if (status === "running") {
    return "running";
  }
  if (status === "failed" || status === "error") {
    return "failed";
  }
  return "completed";
}

function toToolTraceItems(
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

type ThinkingRenderSegment = {
  key: string;
  kind: "thinking";
  text: string;
  isStreaming: boolean;
  isActive: boolean;
  defaultOpen?: boolean;
  durationMs?: number;
};

type ExecutionRenderSegment = {
  key: string;
  kind: "execution";
  tools: ToolCallTraceItem[];
  subagentItems: ReactNode[];
  subagentCount: number;
  defaultOpen: boolean;
};

type TextRenderSegment = { key: string; kind: "text"; text: string };

type RenderSegment =
  ThinkingRenderSegment | ExecutionRenderSegment | TextRenderSegment;

const RenderSegments = memo(function RenderSegments({
  sessionId,
  segments,
}: {
  sessionId: string;
  segments: RenderSegment[];
}) {
  return (
    <div className="w-full space-y-1">
      {segments.map((segment) => {
        if (segment.kind === "thinking") {
          return (
            <ThinkingTrace
              key={segment.key}
              content={segment.text}
              isStreaming={segment.isStreaming}
              isActive={segment.isActive}
              defaultOpen={segment.defaultOpen}
              durationMs={segment.durationMs}
            />
          );
        }
        if (segment.kind === "text") {
          return (
            <div key={segment.key} className={`${CHAT_PROSE} pt-1`}>
              <Streamdown controls={STREAMDOWN_CONTROLS}>
                {segment.text}
              </Streamdown>
            </div>
          );
        }
        if (segment.tools.length === 0 && segment.subagentCount === 0) {
          return null;
        }
        const summaryParts: string[] = [];
        if (segment.tools.length > 0) {
          summaryParts.push(`${segment.tools.length} 个工具`);
        }
        if (segment.subagentCount > 0) {
          summaryParts.push(`${segment.subagentCount} 个子代理`);
        }
        return (
          <Trace key={segment.key}>
            <TraceCollapsible defaultOpen={segment.defaultOpen}>
              <TraceCollapsibleTrigger>
                <TraceLine className="h-6 text-xs leading-5">
                  <TraceIcon>
                    <ListTreeIcon className="size-3.5" />
                  </TraceIcon>
                  <TraceTitle>执行详情</TraceTitle>
                  <TraceDescription className="text-[11px]">
                    {summaryParts.join(" · ")}
                  </TraceDescription>
                </TraceLine>
              </TraceCollapsibleTrigger>
              <TraceCollapsibleContent className="mt-1.5 space-y-1.5 border-l pl-4">
                <ToolCallList sessionId={sessionId} tools={segment.tools} />
                {segment.subagentItems}
              </TraceCollapsibleContent>
            </TraceCollapsible>
          </Trace>
        );
      })}
    </div>
  );
});

function HistoricalSubagent({
  sessionId,
  subagent,
}: {
  sessionId: string;
  subagent: RunTraceSubagent;
}) {
  const title = subagent.display_name?.trim() || subagent.agent_name;
  const subTools = toToolTraceItems(subagent.tools);
  const filteredTools = subTools.filter((t) => t.toolName !== "describe_task");
  return (
    <Trace>
      <TraceCollapsible defaultOpen>
        <TraceCollapsibleTrigger>
          <TraceLine
            className="h-6 text-xs leading-5"
            status={subagent.status === "failed" ? "error" : undefined}
          >
            <TraceIcon>
              <UsersIcon className="size-3.5" />
            </TraceIcon>
            <TraceTitle>{title}</TraceTitle>
            {subagent.task_focus?.title && (
              <TraceDescription className="truncate text-[11px]">
                {subagent.task_focus.title}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
          <ThinkingTrace
            content={subagent.thinking}
            isStreaming={false}
            defaultOpen={false}
          />
          {filteredTools.length > 0 && (
            <Trace>
              <TraceCollapsible defaultOpen={false}>
                <TraceCollapsibleTrigger>
                  <TraceLine className="h-6 text-xs leading-5">
                    <TraceIcon>
                      <ListTreeIcon className="size-3.5" />
                    </TraceIcon>
                    <TraceTitle>执行详情</TraceTitle>
                    <TraceDescription className="text-[11px]">
                      {filteredTools.length} 个工具
                    </TraceDescription>
                  </TraceLine>
                </TraceCollapsibleTrigger>
                <TraceCollapsibleContent className="mt-1.5 space-y-1.5 border-l pl-4">
                  <ToolCallList sessionId={sessionId} tools={subTools} />
                </TraceCollapsibleContent>
              </TraceCollapsible>
            </Trace>
          )}
          {subagent.summary && (
            <div className="text-xs text-muted-foreground">
              {subagent.summary}
            </div>
          )}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

function buildHistoricalSegments(
  sessionId: string,
  trace: RunTraceSummary,
): RenderSegment[] {
  const source = trace.segments ?? [];
  const lastThinkingIndex = source.reduce(
    (last, segment, index) => (segment.kind === "thinking" ? index : last),
    -1,
  );
  const segments: RenderSegment[] = [];
  source.forEach((segment, index) => {
    if (segment.kind === "thinking") {
      if (!segment.text.trim()) {
        return;
      }
      segments.push({
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
      segments.push({
        key: `text-${index}`,
        kind: "text",
        text: segment.text,
      });
      return;
    }
    const tools = toToolTraceItems(segment.tools);
    const subagents = segment.subagents ?? [];
    if (tools.length === 0 && subagents.length === 0) {
      return;
    }
    segments.push({
      key: `execution-${index}`,
      kind: "execution",
      tools,
      subagentItems: subagents.map((subagent) => (
        <HistoricalSubagent
          key={subagent.subagent_run_id}
          sessionId={sessionId}
          subagent={subagent}
        />
      )),
      subagentCount: subagents.length,
      defaultOpen: false,
    });
  });
  return segments;
}

function HistoricalRunTrace({
  sessionId,
  trace,
}: {
  sessionId: string;
  trace: RunTraceSummary;
}) {
  const segments = buildHistoricalSegments(sessionId, trace);
  if (segments.length === 0) {
    return null;
  }
  return <RenderSegments sessionId={sessionId} segments={segments} />;
}

const HistoricalMessageItem = memo(function HistoricalMessageItem({
  sessionId,
  msg,
  attachmentMap,
  trace,
}: {
  sessionId: string;
  msg: SessionMessage;
  attachmentMap: Map<string, AttachmentMeta>;
  trace?: RunTraceSummary;
}) {
  const msgAttachmentIds = msg.attachment_ids ?? [];
  const sentAt = formatMsgTime(msg.created_at);
  const hasTextSegment =
    trace?.segments?.some(
      (segment) => segment.kind === "text" && segment.text.trim().length > 0,
    ) ?? false;
  const textSegmentContent =
    trace?.segments
      ?.filter((segment) => segment.kind === "text")
      .map((segment) => segment.text)
      .join("") ?? "";
  const copyContent =
    msg.content.trim().length > 0 ? msg.content : textSegmentContent;

  if (msg.role === "user") {
    return (
      <MessageGroup className="gap-1">
        {sentAt && (
          <time
            dateTime={msg.created_at}
            title={sentAt.full}
            className="self-end px-1 text-[11px] leading-none text-muted-foreground/70 tabular-nums"
          >
            {sentAt.label}
          </time>
        )}
        <Message align="end" className="items-end">
          <div className="flex max-w-[85%] flex-col items-end">
            {msgAttachmentIds.length > 0 && (
              <AttachmentGroup className="mb-1.5 justify-end">
                {msgAttachmentIds.map((attId) => {
                  const meta = attachmentMap.get(attId);
                  return (
                    <Attachment key={attId} size="xs">
                      <AttachmentMedia variant="icon">
                        <FileTextIcon className="size-3.5" />
                      </AttachmentMedia>
                      <AttachmentContent>
                        <AttachmentTitle>
                          {meta?.filename ?? attId}
                        </AttachmentTitle>
                        {meta && (
                          <AttachmentDescription>
                            {formatBytes(meta.size_bytes)}
                          </AttachmentDescription>
                        )}
                      </AttachmentContent>
                    </Attachment>
                  );
                })}
              </AttachmentGroup>
            )}
            <div className="w-fit max-w-full rounded-2xl bg-muted px-4 py-2.5 text-sm leading-6 whitespace-pre-wrap text-foreground">
              {msg.content}
            </div>
          </div>
        </Message>
      </MessageGroup>
    );
  }

  return (
    <MessageGroup className="group/assistant">
      <Message align="start" className="flex-col items-start">
        {trace && <HistoricalRunTrace sessionId={sessionId} trace={trace} />}
        {!hasTextSegment && (
          <MessageContent className={CHAT_PROSE}>
            <Streamdown controls={STREAMDOWN_CONTROLS}>
              {msg.content}
            </Streamdown>
          </MessageContent>
        )}
        <div className="-mt-1 flex w-full items-center gap-2">
          {sentAt && (
            <time
              dateTime={msg.created_at}
              title={sentAt.full}
              className="px-1 text-[11px] text-muted-foreground/60 tabular-nums"
            >
              {sentAt.label}
            </time>
          )}
          <div className="ml-auto flex items-center gap-1">
            <CopyMessageButton content={copyContent} />
          </div>
        </div>
      </Message>
    </MessageGroup>
  );
});

function LiveSubagent({
  sessionId,
  sub,
}: {
  sessionId: string;
  sub: LiveSubagentRun;
}) {
  const segments = buildLiveSegments(sessionId, sub.trace, [], {
    defaultCollapsed: true,
  });
  const title = sub.displayName?.trim() || sub.agentName;
  return (
    <Trace>
      <TraceCollapsible defaultOpen>
        <TraceCollapsibleTrigger>
          <TraceLine
            className="h-6 text-xs leading-5"
            status={
              sub.status === "running"
                ? "running"
                : sub.status === "failed"
                  ? "error"
                  : undefined
            }
          >
            <TraceIcon>
              {sub.status === "running" ? (
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              ) : (
                <UsersIcon className="size-3.5" />
              )}
            </TraceIcon>
            <TraceTitle>{title}</TraceTitle>
            {sub.trace.taskFocus?.title && (
              <TraceDescription className="truncate text-[11px]">
                {sub.trace.taskFocus.title}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
          <RenderSegments sessionId={sessionId} segments={segments} />
          {sub.summary && (
            <div className="text-xs text-muted-foreground">{sub.summary}</div>
          )}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

function buildLiveSegments(
  sessionId: string,
  trace: LiveAgentTrace,
  subagents: LiveSubagentRun[],
  options?: { defaultCollapsed?: boolean },
): RenderSegment[] {
  const lastIndex = trace.segments.length - 1;
  const segments: RenderSegment[] = [];
  trace.segments.forEach((segment: LiveTraceSegment, index) => {
    const isLast = index === lastIndex;
    if (segment.kind === "thinking") {
      if (!segment.text.trim()) {
        return;
      }
      segments.push({
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
      segments.push({
        key: `text-${index}`,
        kind: "text",
        text: segment.text,
      });
      return;
    }
    const runs = segment.subagentRunIds
      .map((id) => subagents.find((sub) => sub.subagentRunId === id))
      .filter((sub): sub is LiveSubagentRun => sub !== undefined);
    if (segment.tools.length === 0 && runs.length === 0) {
      return;
    }
    segments.push({
      key: `execution-${index}`,
      kind: "execution",
      tools: segment.tools,
      subagentItems: runs.map((sub) => (
        <LiveSubagent key={sub.subagentRunId} sessionId={sessionId} sub={sub} />
      )),
      subagentCount: runs.length,
      defaultOpen: !options?.defaultCollapsed,
    });
  });
  return segments;
}

function LiveTraceSegments({
  sessionId,
  trace,
  subagents,
}: {
  sessionId: string;
  trace: LiveAgentTrace;
  subagents: LiveSubagentRun[];
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={buildLiveSegments(sessionId, trace, subagents)}
    />
  );
}

function AutoFollowOnSend({
  lastUserTurnKey,
  hasLiveMain,
}: {
  lastUserTurnKey: string | null;
  hasLiveMain: boolean;
}) {
  const { scrollToEnd } = useMessageScroller();
  const prevTurnKeyRef = useRef<string | null>(lastUserTurnKey);
  const prevLiveRef = useRef<boolean>(hasLiveMain);

  useEffect(() => {
    if (
      lastUserTurnKey !== null &&
      lastUserTurnKey !== prevTurnKeyRef.current
    ) {
      scrollToEnd({ behavior: "smooth" });
    }
    prevTurnKeyRef.current = lastUserTurnKey;
  }, [lastUserTurnKey, scrollToEnd]);

  useEffect(() => {
    if (hasLiveMain && !prevLiveRef.current) {
      scrollToEnd({ behavior: "smooth" });
    }
    prevLiveRef.current = hasLiveMain;
  }, [hasLiveMain, scrollToEnd]);

  return null;
}

export function SessionStreamView({
  sessionId,
  messages,
  optimisticMessages = [],
  attachments = [],
  traces = [],
  liveMain,
  liveSubagents,
  pendingQuestion,
  resolvedQuestions = [],
  isAnsweringQuestion,
  onAnswerQuestion,
  onJumpToSection,
  onInterrupt,
  contentClassName,
}: SessionStreamViewProps) {
  const attachmentMap = useMemo(() => {
    const map = new Map<string, AttachmentMeta>();
    for (const item of attachments) {
      map.set(item.attachment_id, item);
    }
    return map;
  }, [attachments]);

  const traceByRunId = useMemo(() => {
    const map = new Map<string, RunTraceSummary>();
    for (const trace of traces) {
      map.set(trace.run_id, trace);
    }
    return map;
  }, [traces]);

  const visibleMessages = useMemo(() => {
    const persisted = messages.filter(
      (msg) => msg.role === "user" || msg.role === "assistant",
    );
    if (optimisticMessages.length === 0) {
      return persisted;
    }
    const persistedContents = new Set(
      persisted.map((m) => `${m.role}:${m.content}`),
    );
    const pending = optimisticMessages.filter(
      (opt) => !persistedContents.has(`${opt.role}:${opt.content}`),
    );
    return [...persisted, ...pending];
  }, [messages, optimisticMessages]);

  const lastUserTurnKey = useMemo(() => {
    let userCount = 0;
    let lastContent: string | null = null;
    for (const msg of visibleMessages) {
      if (msg.role === "user") {
        userCount += 1;
        lastContent = msg.content;
      }
    }
    return lastContent !== null ? `${userCount}:${lastContent}` : null;
  }, [visibleMessages]);

  const allQuestionCards = useMemo(() => {
    const byId = new Map<
      string,
      {
        question: PendingQuestion;
        resolvedState?: {
          status: "answered" | "skipped" | "dismissed";
          answers?: QuestionAnswer[];
        };
      }
    >();
    for (const item of resolvedQuestions) {
      byId.set(item.question.question_id, {
        question: item.question,
        resolvedState: { status: item.status, answers: item.answers },
      });
    }
    if (pendingQuestion) {
      byId.set(pendingQuestion.question_id, { question: pendingQuestion });
    }
    return Array.from(byId.values());
  }, [pendingQuestion, resolvedQuestions]);

  const questionsByRunId = useMemo(() => {
    const map = new Map<string, typeof allQuestionCards>();
    for (const entry of allQuestionCards) {
      const list = map.get(entry.question.run_id) ?? [];
      list.push(entry);
      map.set(entry.question.run_id, list);
    }
    return map;
  }, [allQuestionCards]);

  const anchoredQuestionIds = useMemo(() => {
    const ids = new Set<string>();
    for (const msg of visibleMessages) {
      if (msg.role === "assistant" && msg.run_id) {
        const matched = questionsByRunId.get(msg.run_id);
        if (matched) {
          for (const entry of matched) {
            ids.add(entry.question.question_id);
          }
        }
      }
    }
    return ids;
  }, [visibleMessages, questionsByRunId]);

  const unanchoredQuestions = useMemo(
    () =>
      allQuestionCards.filter(
        (entry) => !anchoredQuestionIds.has(entry.question.question_id),
      ),
    [allQuestionCards, anchoredQuestionIds],
  );

  const renderQuestionCard = (entry: (typeof allQuestionCards)[number]) => (
    <QuestionCard
      key={entry.question.question_id}
      pending={entry.question}
      resolvedState={entry.resolvedState}
      isSubmitting={entry.resolvedState ? false : isAnsweringQuestion}
      onSubmit={(answers, skipped) => {
        if (!entry.resolvedState) {
          onAnswerQuestion(entry.question.question_id, answers, skipped);
        }
      }}
    />
  );

  return (
    <MessageScrollerProvider autoScroll scrollEdgeThreshold={24}>
      <AutoFollowOnSend
        lastUserTurnKey={lastUserTurnKey}
        hasLiveMain={liveMain !== null}
      />
      <MessageScroller className="h-full">
        <MessageScrollerViewport className="px-4">
          <MessageScrollerContent className={`${contentClassName} py-4`}>
            {visibleMessages.map((msg) => {
              const attachedQuestions =
                msg.role === "assistant" && msg.run_id
                  ? questionsByRunId.get(msg.run_id)
                  : undefined;
              return (
                <MessageScrollerItem
                  key={msg.message_id}
                  messageId={msg.message_id}
                  scrollAnchor={msg.role === "user"}
                >
                  <HistoricalMessageItem
                    sessionId={sessionId}
                    msg={msg}
                    attachmentMap={attachmentMap}
                    trace={
                      msg.run_id ? traceByRunId.get(msg.run_id) : undefined
                    }
                  />
                  {attachedQuestions?.map(renderQuestionCard)}
                </MessageScrollerItem>
              );
            })}

            {liveMain && (
              <MessageScrollerItem messageId="live-main">
                <MessageGroup>
                  <Message align="start">
                    <MessageContent className="w-full space-y-1">
                      {liveMain.isStreaming && (
                        <div className="flex h-6 w-full items-center justify-between gap-2 text-xs leading-5 text-muted-foreground">
                          <button
                            type="button"
                            onClick={() => {
                              if (liveMain.taskFocus?.sectionId) {
                                onJumpToSection?.(liveMain.taskFocus.sectionId);
                              }
                            }}
                            className="flex min-w-0 flex-1 items-center gap-2 text-left"
                          >
                            <span className="flex size-4 shrink-0 items-center justify-center">
                              <SparklesIcon className="size-3.5 text-primary" />
                            </span>
                            <span className="shimmer truncate font-medium">
                              {liveMain.taskFocus?.title ?? "正在处理"}
                            </span>
                            {liveMain.taskFocus?.detail && (
                              <span className="truncate opacity-75">
                                — {liveMain.taskFocus.detail}
                              </span>
                            )}
                          </button>
                          {onInterrupt && (
                            <button
                              type="button"
                              onClick={() => {
                                void onInterrupt();
                              }}
                              className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-xs font-medium text-destructive/85 transition-colors hover:bg-destructive/10 hover:text-destructive"
                            >
                              <SquareIcon className="size-2.5 fill-current" />
                              <span>停止</span>
                            </button>
                          )}
                        </div>
                      )}

                      {!liveMain.isStreaming && liveMain.taskFocus && (
                        <button
                          type="button"
                          onClick={() => {
                            if (liveMain.taskFocus?.sectionId) {
                              onJumpToSection?.(liveMain.taskFocus.sectionId);
                            }
                          }}
                          className="flex w-fit items-center gap-1.5 rounded-full border bg-muted/50 px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                        >
                          <CompassIcon className="size-3.5 text-primary" />
                          <span className="font-medium">
                            {liveMain.taskFocus.title}
                          </span>
                        </button>
                      )}

                      <LiveTraceSegments
                        sessionId={sessionId}
                        trace={liveMain}
                        subagents={liveSubagents}
                      />
                    </MessageContent>
                  </Message>
                </MessageGroup>
              </MessageScrollerItem>
            )}

            {unanchoredQuestions.map((entry) => (
              <MessageScrollerItem
                key={`question-${entry.question.question_id}`}
                messageId={`question-${entry.question.question_id}`}
              >
                {renderQuestionCard(entry)}
              </MessageScrollerItem>
            ))}
          </MessageScrollerContent>
        </MessageScrollerViewport>
        <MessageScrollerButton />
      </MessageScroller>
    </MessageScrollerProvider>
  );
}

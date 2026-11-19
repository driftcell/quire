import {
  Fragment,
  memo,
  useCallback,
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
import { parseCommentRefs, type CommentRefPayload } from "@/lib/commentRefXml";

const CHAT_PROSE =
  "prose prose-sm dark:prose-invert max-w-none prose-headings:font-semibold prose-headings:mt-3 prose-headings:mb-1.5 prose-h1:text-base prose-h2:text-[15px] prose-h3:text-sm prose-p:my-1.5 prose-p:leading-6 prose-ul:my-1.5 prose-ol:my-1.5 prose-pre:my-2 prose-pre:text-xs prose-code:text-[12.5px]";

const STREAMDOWN_CONTROLS: ControlsConfig = {
  table: false,
  code: true,
};

export interface SessionStreamViewProps {
  sessionId: string;
  messages: SessionMessage[];
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
  questionSlots,
}: {
  sessionId: string;
  tools: ToolCallTraceItem[];
  questionSlots?: Map<string, ReactNode>;
}) {
  const [expanded, setExpanded] = useState(false);
  const tools = useMemo(
    () => rawTools.filter((t) => t.toolName !== "describe_task"),
    [rawTools],
  );

  const renderTool = (tool: ToolCallTraceItem) => (
    <Fragment key={tool.toolCallId}>
      <ToolCallTrace sessionId={sessionId} item={tool} />
      {questionSlots?.get(tool.toolCallId)}
    </Fragment>
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
        {tools.map(renderTool)}
      </div>
    );
  }

  const splitAt = tools.length - VISIBLE_RECENT_TOOLS;
  const hiddenTools = tools.slice(0, splitAt);
  const recentTools = tools.slice(splitAt);
  const pinnedTools = hiddenTools.filter((tool) =>
    questionSlots?.has(tool.toolCallId),
  );
  const restHidden = hiddenTools.filter(
    (tool) => !questionSlots?.has(tool.toolCallId),
  );
  const runningHidden = restHidden.filter((t) => t.status === "running");

  return (
    <div className="space-y-1">
      {restHidden.length > 0 && (
        <button
          type="button"
          onClick={() => setExpanded(true)}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-muted-foreground tabular-nums hover:bg-muted hover:text-foreground"
        >
          <ChevronDownIcon className="size-3" />
          <span>
            显示更早的 {restHidden.length} 个调用（共 {tools.length} 个）
          </span>
        </button>
      )}
      {pinnedTools.map(renderTool)}
      {runningHidden.map(renderTool)}
      {recentTools.map(renderTool)}
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

type ThinkingActivity = {
  key: string;
  kind: "thinking";
  text: string;
  isStreaming: boolean;
  isActive: boolean;
  defaultOpen?: boolean;
  durationMs?: number;
};

type ExecutionActivity = {
  key: string;
  kind: "execution";
  tools: ToolCallTraceItem[];
  subagentItems: ReactNode[];
  subagentCount: number;
  defaultOpen: boolean;
};

type TextActivity = {
  key: string;
  kind: "text";
  text: string;
  isStreaming: boolean;
};

type ActivityItem = ThinkingActivity | ExecutionActivity | TextActivity;

type ActivityRenderSegment = {
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

type RenderSegment = ActivityRenderSegment | TextRenderSegment;

function activityHasQuestion(
  items: ActivityItem[],
  questionSlots?: Map<string, ReactNode>,
) {
  if (!questionSlots) {
    return false;
  }
  return items.some(
    (item) =>
      item.kind === "execution" &&
      item.tools.some((tool) => questionSlots.has(tool.toolCallId)),
  );
}

function ExecutionTrace({
  segmentKey,
  sessionId,
  tools,
  subagentItems,
  subagentCount,
  defaultOpen,
  questionSlots,
}: {
  segmentKey: string;
  sessionId: string;
  tools: ToolCallTraceItem[];
  subagentItems: ReactNode[];
  subagentCount: number;
  defaultOpen: boolean;
  questionSlots?: Map<string, ReactNode>;
}) {
  if (tools.length === 0 && subagentCount === 0) {
    return null;
  }
  const summaryParts: string[] = [];
  if (tools.length > 0) {
    summaryParts.push(`${tools.length} 个工具`);
  }
  if (subagentCount > 0) {
    summaryParts.push(`${subagentCount} 个子代理`);
  }
  return (
    <Trace key={segmentKey}>
      <TraceCollapsible defaultOpen={defaultOpen}>
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
          <ToolCallList
            sessionId={sessionId}
            tools={tools}
            questionSlots={questionSlots}
          />
          {subagentItems}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

function ActivitySegment({
  sessionId,
  segment,
  questionSlots,
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
}) {
  const thinkingItems = segment.items.filter(
    (item): item is ThinkingActivity => item.kind === "thinking",
  );
  const executionItems = segment.items.filter(
    (item): item is ExecutionActivity => item.kind === "execution",
  );
  const textItems = segment.items.filter(
    (item): item is TextActivity => item.kind === "text",
  );
  const hasQuestion = activityHasQuestion(segment.items, questionSlots);

  if (executionItems.length === 0 && textItems.length === 0) {
    return (
      <>
        {thinkingItems.map((item) => (
          <ThinkingTrace
            key={item.key}
            content={item.text}
            isStreaming={item.isStreaming}
            isActive={item.isActive}
            defaultOpen={item.defaultOpen}
            durationMs={item.durationMs}
          />
        ))}
      </>
    );
  }

  if (thinkingItems.length === 0 && textItems.length === 0) {
    return (
      <ExecutionTrace
        segmentKey={segment.key}
        sessionId={sessionId}
        tools={executionItems.flatMap((item) => item.tools)}
        subagentItems={executionItems.flatMap((item) => item.subagentItems)}
        subagentCount={executionItems.reduce(
          (total, item) => total + item.subagentCount,
          0,
        )}
        defaultOpen={
          hasQuestion || executionItems.some((item) => item.defaultOpen)
        }
        questionSlots={questionSlots}
      />
    );
  }

  const tools = executionItems.flatMap((item) => item.tools);
  const subagentCount = executionItems.reduce(
    (total, item) => total + item.subagentCount,
    0,
  );
  const summaryParts: string[] = [];
  if (tools.length > 0) {
    summaryParts.push(`${tools.length} 个工具`);
  }
  if (subagentCount > 0) {
    summaryParts.push(`${subagentCount} 个子代理`);
  }
  const streaming = segment.items.some(
    (item) =>
      (item.kind === "thinking" || item.kind === "text") && item.isStreaming,
  );
  const title =
    tools.length > 0 || subagentCount > 0
      ? thinkingItems.length > 0
        ? "思考与执行"
        : "执行详情"
      : "思考过程";

  return (
    <Trace key={segment.key}>
      <TraceCollapsible
        defaultOpen={
          hasQuestion ||
          streaming ||
          executionItems.some((item) => item.defaultOpen)
        }
      >
        <TraceCollapsibleTrigger>
          <TraceLine
            status={streaming ? "running" : undefined}
            className="h-6 text-xs leading-5"
          >
            <TraceIcon>
              {streaming ? (
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              ) : (
                <ListTreeIcon className="size-3.5" />
              )}
            </TraceIcon>
            <TraceTitle>{streaming ? "正在处理" : title}</TraceTitle>
            <TraceDescription className="text-[11px]">
              {summaryParts.join(" · ")}
            </TraceDescription>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1.5 space-y-2 border-l pl-4">
          {segment.items.map((item) => {
            if (item.kind === "thinking") {
              return (
                <ThinkingTrace
                  key={item.key}
                  content={item.text}
                  isStreaming={item.isStreaming}
                  isActive={item.isActive}
                  defaultOpen={item.defaultOpen}
                  durationMs={item.durationMs}
                />
              );
            }
            if (item.kind === "text") {
              return (
                <div key={item.key} className={`${CHAT_PROSE} text-sm`}>
                  <Streamdown
                    mode={item.isStreaming ? "streaming" : "static"}
                    controls={STREAMDOWN_CONTROLS}
                  >
                    {item.text}
                  </Streamdown>
                </div>
              );
            }
            return (
              <Fragment key={item.key}>
                <ToolCallList
                  sessionId={sessionId}
                  tools={item.tools}
                  questionSlots={questionSlots}
                />
                {item.subagentItems}
              </Fragment>
            );
          })}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

const RenderSegments = memo(function RenderSegments({
  sessionId,
  segments,
  questionSlots,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
}) {
  return (
    <div className="w-full space-y-1">
      {segments.map((segment) => {
        if (segment.kind === "text") {
          return (
            <div key={segment.key} className={`${CHAT_PROSE} pt-1`}>
              <Streamdown
                mode={segment.isStreaming ? "streaming" : "static"}
                controls={STREAMDOWN_CONTROLS}
              >
                {segment.text}
              </Streamdown>
            </div>
          );
        }
        return (
          <ActivitySegment
            key={segment.key}
            sessionId={sessionId}
            segment={segment}
            questionSlots={questionSlots}
          />
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

function HistoricalRunTrace({
  sessionId,
  trace,
  questionSlots,
}: {
  sessionId: string;
  trace: RunTraceSummary;
  questionSlots?: Map<string, ReactNode>;
}) {
  const segments = buildHistoricalSegments(sessionId, trace);
  if (segments.length === 0) {
    return null;
  }
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={segments}
      questionSlots={questionSlots}
    />
  );
}

function UserCommentRefCards({
  refs,
  onJumpToSection,
}: {
  refs: CommentRefPayload[];
  onJumpToSection?: (sectionId: string) => void;
}) {
  return (
    <div className="mb-1.5 flex w-full flex-col gap-1.5">
      {refs.map((ref) => (
        <button
          key={ref.thread_id}
          type="button"
          onClick={() => onJumpToSection?.(ref.block_id)}
          className="w-full cursor-pointer rounded-md border-l-2 border-border/80 bg-muted/35 px-3 py-2 text-left text-xs transition-colors hover:bg-muted/55 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <div className="line-clamp-1 text-[11px] text-foreground">
            {ref.quote ? `“${ref.quote}”` : ref.comment}
          </div>
          {ref.quote && (
            <div className="mt-0.5 line-clamp-1 text-[10px] text-muted-foreground">
              {ref.comment}
            </div>
          )}
        </button>
      ))}
    </div>
  );
}

const HistoricalMessageItem = memo(function HistoricalMessageItem({
  sessionId,
  msg,
  attachmentMap,
  trace,
  questionSlots,
  onJumpToSection,
}: {
  sessionId: string;
  msg: SessionMessage;
  attachmentMap: Map<string, AttachmentMeta>;
  trace?: RunTraceSummary;
  questionSlots?: Map<string, ReactNode>;
  onJumpToSection?: (sectionId: string) => void;
}) {
  const msgAttachmentIds = msg.attachment_ids ?? [];
  const sentAt = formatMsgTime(msg.created_at);
  const parsedUserContent =
    msg.role === "user" ? parseCommentRefs(msg.content) : null;
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
    const userRefs = parsedUserContent?.refs ?? [];
    const userBody = parsedUserContent?.body ?? msg.content;
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
            {userRefs.length > 0 && (
              <div className="w-full">
                <UserCommentRefCards
                  refs={userRefs}
                  onJumpToSection={onJumpToSection}
                />
              </div>
            )}
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
            {userBody.length > 0 && (
              <div className="w-fit max-w-full rounded-2xl bg-muted px-4 py-2.5 text-sm leading-6 whitespace-pre-wrap text-foreground">
                {userBody}
              </div>
            )}
          </div>
        </Message>
      </MessageGroup>
    );
  }

  return (
    <MessageGroup className="group/assistant">
      <Message align="start" className="flex-col items-start">
        {trace && (
          <HistoricalRunTrace
            sessionId={sessionId}
            trace={trace}
            questionSlots={questionSlots}
          />
        )}
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
      subagentItems: runs.map((sub) => (
        <LiveSubagent key={sub.subagentRunId} sessionId={sessionId} sub={sub} />
      )),
      subagentCount: runs.length,
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

function LiveTraceSegments({
  sessionId,
  trace,
  subagents,
  questionSlots,
}: {
  sessionId: string;
  trace: LiveAgentTrace;
  subagents: LiveSubagentRun[];
  questionSlots?: Map<string, ReactNode>;
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={buildLiveSegments(sessionId, trace, subagents)}
      questionSlots={questionSlots}
    />
  );
}

export function SessionStreamView({
  sessionId,
  messages,
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

  const answerQuestionRef = useRef(onAnswerQuestion);
  useEffect(() => {
    answerQuestionRef.current = onAnswerQuestion;
  }, [onAnswerQuestion]);

  const renderQuestionCard = useCallback(
    (entry: (typeof allQuestionCards)[number]) => (
      <QuestionCard
        key={entry.question.question_id}
        pending={entry.question}
        resolvedState={entry.resolvedState}
        isSubmitting={entry.resolvedState ? false : isAnsweringQuestion}
        onSubmit={(answers, skipped) => {
          if (!entry.resolvedState) {
            answerQuestionRef.current(
              entry.question.question_id,
              answers,
              skipped,
            );
          }
        }}
      />
    ),
    [isAnsweringQuestion],
  );

  const questionSlots = useMemo(() => {
    const map = new Map<string, ReactNode>();
    for (const entry of allQuestionCards) {
      const toolCallId = entry.question.tool_call_id;
      if (toolCallId) {
        map.set(toolCallId, renderQuestionCard(entry));
      }
    }
    return map;
  }, [allQuestionCards, renderQuestionCard]);

  const visibleMessages = useMemo(() => {
    const persisted = messages.filter(
      (msg) =>
        msg.role === "user" || msg.role === "assistant" || msg.role === "tool",
    );
    const assistantRunIds = new Set(
      persisted
        .filter((msg) => msg.role === "assistant" && msg.run_id)
        .map((msg) => msg.run_id),
    );
    return persisted.filter((msg) => {
      if (msg.role !== "tool") {
        return true;
      }
      if (!msg.run_id) {
        return false;
      }
      return (
        !assistantRunIds.has(msg.run_id) &&
        questionsByRunId.has(msg.run_id) &&
        traceByRunId.has(msg.run_id)
      );
    });
  }, [messages, questionsByRunId, traceByRunId]);

  const lastUserMessageId = useMemo(() => {
    for (let index = visibleMessages.length - 1; index >= 0; index -= 1) {
      const message = visibleMessages[index];
      if (message?.role === "user") {
        return message.message_id;
      }
    }
    return null;
  }, [visibleMessages]);

  const orphanQuestions = useMemo(
    () =>
      allQuestionCards.filter((entry) => {
        // Every new question carries its ask_user tool-call ID. Rendering it at
        // the bottom while its trace is being fetched causes a visible jump, so
        // wait for the trace instead. Only legacy rows without an ID fall back.
        return !entry.question.tool_call_id;
      }),
    [allQuestionCards],
  );

  return (
    <MessageScrollerProvider
      autoScroll
      defaultScrollPosition="last-anchor"
      scrollEdgeThreshold={24}
    >
      <MessageScroller className="h-full">
        <MessageScrollerViewport className="px-4">
          <MessageScrollerContent className={`${contentClassName} py-4`}>
            {visibleMessages.map((msg) => {
              const trace = msg.run_id
                ? traceByRunId.get(msg.run_id)
                : undefined;
              if (msg.role === "tool") {
                if (!trace) {
                  return null;
                }
                return (
                  <MessageScrollerItem
                    key={msg.message_id}
                    messageId={msg.message_id}
                  >
                    <HistoricalRunTrace
                      sessionId={sessionId}
                      trace={trace}
                      questionSlots={questionSlots}
                    />
                  </MessageScrollerItem>
                );
              }
              return (
                <MessageScrollerItem
                  key={msg.message_id}
                  messageId={msg.message_id}
                  scrollAnchor={msg.message_id === lastUserMessageId}
                >
                  <HistoricalMessageItem
                    sessionId={sessionId}
                    msg={msg}
                    attachmentMap={attachmentMap}
                    trace={trace}
                    questionSlots={questionSlots}
                    onJumpToSection={onJumpToSection}
                  />
                </MessageScrollerItem>
              );
            })}

            {liveMain && (
              <MessageScrollerItem messageId="live-main">
                <MessageGroup>
                  <Message align="start">
                    <MessageContent className="w-full gap-1">
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
                        questionSlots={questionSlots}
                      />
                    </MessageContent>
                  </Message>
                </MessageGroup>
              </MessageScrollerItem>
            )}

            {orphanQuestions.map((entry) => (
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

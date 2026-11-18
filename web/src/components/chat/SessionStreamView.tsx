import { memo, useMemo, useState } from "react";
import {
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  ChevronUpIcon,
  CompassIcon,
  CopyIcon,
  FileTextIcon,
  ListTreeIcon,
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
import { Badge } from "@/components/ui/badge";
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
  RunTraceSummary,
  RunTraceToolItem,
  SessionMessage,
} from "@/contracts/session-protocol";

const CHAT_PROSE =
  "prose prose-sm dark:prose-invert max-w-none prose-headings:font-semibold prose-headings:mt-3 prose-headings:mb-1.5 prose-h1:text-base prose-h2:text-[15px] prose-h3:text-sm prose-p:my-1.5 prose-p:leading-6 prose-ul:my-1.5 prose-ol:my-1.5 prose-pre:my-2 prose-pre:text-xs prose-code:text-[12.5px]";

const STREAMDOWN_CONTROLS: ControlsConfig = {
  table: false,
  code: true,
};

export interface LiveAgentTrace {
  agentId: string;
  thinking: string;
  text: string;
  taskFocus?: { title: string; detail?: string; sectionId?: string };
  tools: ToolCallTraceItem[];
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

export interface SessionStreamViewProps {
  sessionId: string;
  messages: SessionMessage[];
  optimisticMessages?: SessionMessage[];
  attachments?: AttachmentMeta[];
  traces?: RunTraceSummary[];
  liveMain: LiveAgentTrace | null;
  liveSubagents: LiveSubagentRun[];
  pendingQuestion: PendingQuestion | null;
  resolvedQuestion?: {
    question: PendingQuestion;
    status: "answered" | "skipped" | "dismissed";
    answers?: QuestionAnswer[];
  } | null;
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

function CopyMessageButton({ content }: { content: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    void navigator.clipboard.writeText(content).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    });
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
            onClick={handleCopy}
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
  tools,
}: {
  sessionId: string;
  tools: ToolCallTraceItem[];
}) {
  const [expanded, setExpanded] = useState(false);

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

function HistoricalRunTrace({
  sessionId,
  trace,
}: {
  sessionId: string;
  trace: RunTraceSummary;
}) {
  const tools = toToolTraceItems(trace.tools);
  const subagents = trace.subagents ?? [];
  const hasExecution = tools.length > 0 || subagents.length > 0;
  if (!trace.thinking.trim() && !hasExecution) {
    return null;
  }

  const summaryParts: string[] = [];
  if (tools.length > 0) {
    summaryParts.push(`${tools.length} 个工具`);
  }
  if (subagents.length > 0) {
    summaryParts.push(`${subagents.length} 个子代理`);
  }

  return (
    <div className="w-full space-y-1">
      <ThinkingTrace
        content={trace.thinking}
        isStreaming={false}
        durationMs={trace.duration_ms ?? undefined}
      />
      {hasExecution && (
        <Trace className="my-1">
          <TraceCollapsible defaultOpen={false}>
            <TraceCollapsibleTrigger>
              <TraceLine>
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
              <ToolCallList sessionId={sessionId} tools={tools} />
              {subagents.map((subagent) => (
                <Trace key={subagent.subagent_run_id}>
                  <TraceCollapsible defaultOpen={false}>
                    <TraceCollapsibleTrigger>
                      <TraceLine
                        status={
                          subagent.status === "failed" ? "error" : undefined
                        }
                      >
                        <TraceIcon>
                          <UsersIcon className="size-3.5" />
                        </TraceIcon>
                        <TraceTitle>{subagent.agent_name}</TraceTitle>
                        {subagent.task && (
                          <Badge variant="outline" className="text-[10px]">
                            {subagent.task}
                          </Badge>
                        )}
                      </TraceLine>
                    </TraceCollapsibleTrigger>
                    <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
                      {subagent.task_focus && (
                        <div className="text-xs text-muted-foreground">
                          {subagent.task_focus.title}
                        </div>
                      )}
                      <ThinkingTrace
                        content={subagent.thinking}
                        isStreaming={false}
                      />
                      <ToolCallList
                        sessionId={sessionId}
                        tools={toToolTraceItems(subagent.tools)}
                      />
                      {subagent.summary && (
                        <div className="text-xs text-muted-foreground">
                          {subagent.summary}
                        </div>
                      )}
                    </TraceCollapsibleContent>
                  </TraceCollapsible>
                </Trace>
              ))}
            </TraceCollapsibleContent>
          </TraceCollapsible>
        </Trace>
      )}
    </div>
  );
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
        {sentAt && (
          <time
            dateTime={msg.created_at}
            title={sentAt.full}
            className="mb-1 px-1 text-[11px] text-muted-foreground/60 tabular-nums"
          >
            {sentAt.label}
          </time>
        )}
        {trace && <HistoricalRunTrace sessionId={sessionId} trace={trace} />}
        <MessageContent className={CHAT_PROSE}>
          <Streamdown controls={STREAMDOWN_CONTROLS}>{msg.content}</Streamdown>
        </MessageContent>
        <div className="mt-1 flex items-center gap-1 opacity-0 transition-opacity group-hover/assistant:opacity-100 focus-within:opacity-100">
          <CopyMessageButton content={msg.content} />
        </div>
      </Message>
    </MessageGroup>
  );
});

export function SessionStreamView({
  sessionId,
  messages,
  optimisticMessages = [],
  attachments = [],
  traces = [],
  liveMain,
  liveSubagents,
  pendingQuestion,
  resolvedQuestion,
  isAnsweringQuestion,
  onAnswerQuestion,
  onJumpToSection,
  onInterrupt,
  contentClassName,
}: SessionStreamViewProps) {
  const [detailsOpen, setDetailsOpen] = useState(true);

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
    const recentPersistedContents = new Set(
      persisted.slice(-5).map((m) => `${m.role}:${m.content}`),
    );
    const pending = optimisticMessages.filter(
      (opt) => !recentPersistedContents.has(`${opt.role}:${opt.content}`),
    );
    return [...persisted, ...pending];
  }, [messages, optimisticMessages]);

  const hasExecutionDetails =
    Boolean(liveMain && liveMain.tools.length > 0) || liveSubagents.length > 0;

  return (
    <MessageScrollerProvider autoScroll>
      <MessageScroller className="h-full">
        <MessageScrollerViewport className="h-full px-4">
          <MessageScrollerContent className={contentClassName}>
            {visibleMessages.map((msg) => (
              <MessageScrollerItem
                key={msg.message_id}
                messageId={msg.message_id}
                scrollAnchor={msg.role === "user"}
              >
                <HistoricalMessageItem
                  sessionId={sessionId}
                  msg={msg}
                  attachmentMap={attachmentMap}
                  trace={msg.run_id ? traceByRunId.get(msg.run_id) : undefined}
                />
              </MessageScrollerItem>
            ))}

            {liveMain && (
              <MessageScrollerItem messageId="live-main">
                <MessageGroup>
                  <Message align="start">
                    <MessageContent className="w-full space-y-2">
                      {liveMain.isStreaming && (
                        <div className="flex w-full items-center justify-between gap-2 text-xs text-muted-foreground">
                          <button
                            type="button"
                            onClick={() => {
                              if (liveMain.taskFocus?.sectionId) {
                                onJumpToSection?.(liveMain.taskFocus.sectionId);
                              }
                            }}
                            className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                          >
                            <SparklesIcon className="size-3.5 shrink-0 text-primary" />
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

                      <ThinkingTrace
                        content={liveMain.thinking}
                        isStreaming={
                          liveMain.isStreaming && liveMain.text.length === 0
                        }
                        isActive={liveMain.isStreaming}
                      />

                      {hasExecutionDetails && (
                        <div className="text-xs text-muted-foreground">
                          <button
                            type="button"
                            aria-expanded={detailsOpen}
                            onClick={() => setDetailsOpen((o) => !o)}
                            className="group flex items-center gap-1.5 py-0.5 hover:text-foreground"
                          >
                            <ListTreeIcon className="size-3.5" />
                            <span>执行详情</span>
                            <ChevronRightIcon
                              className={`size-3.5 transition-transform duration-150 ${
                                detailsOpen ? "rotate-90" : ""
                              }`}
                            />
                          </button>

                          {detailsOpen && (
                            <div className="mt-1 space-y-1.5 border-l pl-3">
                              <ToolCallList
                                sessionId={sessionId}
                                tools={liveMain.tools}
                              />

                              {liveSubagents.length > 0 && (
                                <div className="space-y-1.5 pt-0.5">
                                  {liveSubagents.map((sub) => (
                                    <Trace key={sub.subagentRunId}>
                                      <TraceCollapsible
                                        defaultOpen={sub.status === "running"}
                                      >
                                        <TraceCollapsibleTrigger>
                                          <TraceLine
                                            status={
                                              sub.status === "running"
                                                ? "running"
                                                : sub.status === "failed"
                                                  ? "error"
                                                  : undefined
                                            }
                                          >
                                            <TraceIcon>
                                              <UsersIcon className="size-3.5" />
                                            </TraceIcon>
                                            <TraceTitle>
                                              {sub.agentName}
                                            </TraceTitle>
                                            <Badge
                                              variant="outline"
                                              className="text-[10px]"
                                            >
                                              {sub.task}
                                            </Badge>
                                          </TraceLine>
                                        </TraceCollapsibleTrigger>
                                        <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
                                          {sub.trace.taskFocus && (
                                            <div className="text-xs text-muted-foreground">
                                              {sub.trace.taskFocus.title}
                                            </div>
                                          )}
                                          <ThinkingTrace
                                            content={sub.trace.thinking}
                                            isStreaming={
                                              sub.status === "running"
                                            }
                                            isActive={sub.status === "running"}
                                          />
                                          <ToolCallList
                                            sessionId={sessionId}
                                            tools={sub.trace.tools}
                                          />
                                          {sub.summary && (
                                            <div className="text-xs text-muted-foreground">
                                              {sub.summary}
                                            </div>
                                          )}
                                        </TraceCollapsibleContent>
                                      </TraceCollapsible>
                                    </Trace>
                                  ))}
                                </div>
                              )}
                            </div>
                          )}
                        </div>
                      )}

                      {liveMain.text && (
                        <div className={`${CHAT_PROSE} pt-1`}>
                          <Streamdown controls={STREAMDOWN_CONTROLS}>
                            {liveMain.text}
                          </Streamdown>
                        </div>
                      )}
                    </MessageContent>
                  </Message>
                </MessageGroup>
              </MessageScrollerItem>
            )}

            {resolvedQuestion && !pendingQuestion && (
              <MessageScrollerItem messageId="resolved-question">
                <QuestionCard
                  pending={resolvedQuestion.question}
                  resolvedState={{
                    status: resolvedQuestion.status,
                    answers: resolvedQuestion.answers,
                  }}
                  isSubmitting={false}
                  onSubmit={() => {}}
                />
              </MessageScrollerItem>
            )}

            {pendingQuestion && (
              <MessageScrollerItem messageId="pending-question">
                <QuestionCard
                  pending={pendingQuestion}
                  isSubmitting={isAnsweringQuestion}
                  onSubmit={(answers, skipped) =>
                    onAnswerQuestion(
                      pendingQuestion.question_id,
                      answers,
                      skipped,
                    )
                  }
                />
              </MessageScrollerItem>
            )}
          </MessageScrollerContent>
        </MessageScrollerViewport>
        <MessageScrollerButton />
      </MessageScroller>
    </MessageScrollerProvider>
  );
}

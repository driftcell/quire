import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { CompassIcon, SparklesIcon } from "lucide-react";
import {
  HistoricalRunTrace,
  LiveTraceSegments,
} from "@/components/chat/ExecutionTraceSection";
import { HistoricalMessageItem } from "@/components/chat/HistoricalMessageItem";
import { PendingUserBubble } from "@/components/chat/PendingUserBubble";
import { QuestionCard } from "@/components/chat/QuestionCard";
import { StopTurnButton } from "@/components/chat/StopTurnButton";
import {
  buildLiveSegments,
  formatLiveElapsed,
  summarizeLiveProgress,
} from "@/components/chat/traceSegments";
import { Message, MessageContent, MessageGroup } from "@/components/ui/message";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from "@/components/ui/message-scroller";
import type {
  AttachmentMeta,
  PendingQueueItem,
  PendingQuestion,
  QuestionAnswer,
  RunTraceSummary,
  SessionCommentRef,
  SessionMessage,
} from "@/contracts/session-protocol";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

function LiveRunSection({
  sessionId,
  trace,
  subagents,
  segments,
  questionSlots,
  onJumpToSection,
  onInterrupt,
  messageById,
  attachmentMap,
}: {
  sessionId: string;
  trace: LiveAgentTrace;
  subagents: LiveSubagentRun[];
  segments: ReturnType<typeof buildLiveSegments>;
  questionSlots: Map<string, ReactNode>;
  onJumpToSection?: (sectionId: string) => void;
  onInterrupt?: () => Promise<void>;
  messageById: ReadonlyMap<string, SessionMessage>;
  attachmentMap: Map<string, AttachmentMeta>;
}) {
  const [detailsOpen, setDetailsOpen] = useState(true);
  const [startedAt] = useState(() => Date.now());
  const [now, setNow] = useState(startedAt);

  useEffect(() => {
    const interval = trace.isStreaming
      ? window.setInterval(() => setNow(Date.now()), 1000)
      : null;
    return () => {
      if (interval !== null) {
        window.clearInterval(interval);
      }
    };
  }, [trace.isStreaming]);

  const progress = summarizeLiveProgress(trace, subagents);
  const elapsed = Math.max(0, Math.floor((now - startedAt) / 1000));

  return (
    <>
      {trace.isStreaming && (
        <div
          data-live-header="true"
          className="flex h-6 w-full items-center justify-between gap-1 text-xs leading-5 text-muted-foreground"
        >
          <button
            type="button"
            data-live-header-toggle="true"
            aria-expanded={detailsOpen}
            title={detailsOpen ? "折叠执行过程" : "展开执行过程"}
            onClick={() => setDetailsOpen((open) => !open)}
            className="flex min-w-0 flex-1 items-center gap-2 text-left"
          >
            <span
              data-live-header-icon="sparkles"
              className="flex size-4 shrink-0 items-center justify-center"
            >
              <SparklesIcon className="size-3.5 text-primary" />
            </span>
            <span className="shimmer min-w-0 truncate font-medium">
              {trace.taskFocus?.title ?? "正在处理"}
            </span>
            {trace.taskFocus?.detail && (
              <span className="min-w-0 truncate text-[11px] opacity-75">
                — {trace.taskFocus.detail}
              </span>
            )}
            <span className="ml-auto flex shrink-0 items-center gap-2 text-[11px] tabular-nums text-muted-foreground/75">
              {progress.toolCount > 0 && (
                <span data-live-progress-tools="true">
                  {`工具 ${progress.completedToolCount}/${progress.toolCount}`}
                </span>
              )}
              {progress.subagentCount > 0 && (
                <span data-live-progress-subagents="true">
                  {`子代理 ${progress.completedSubagentCount}/${progress.subagentCount}`}
                </span>
              )}
              <span data-live-elapsed="true">{formatLiveElapsed(elapsed)}</span>
            </span>
          </button>
          {trace.taskFocus?.sectionId && onJumpToSection && (
            <button
              type="button"
              data-live-header-jump="true"
              aria-label="跳转到当前章节"
              title="跳转到当前章节"
              onClick={() => {
                if (trace.taskFocus?.sectionId) {
                  onJumpToSection(trace.taskFocus.sectionId);
                }
              }}
              className="inline-flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <CompassIcon className="size-3.5" aria-hidden="true" />
            </button>
          )}
          {onInterrupt && <StopTurnButton onInterrupt={onInterrupt} />}
        </div>
      )}

      {!trace.isStreaming && trace.taskFocus && (
        <button
          type="button"
          onClick={() => {
            if (trace.taskFocus?.sectionId) {
              onJumpToSection?.(trace.taskFocus.sectionId);
            }
          }}
          className="flex w-fit items-center gap-1.5 rounded-full border bg-muted/50 px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <CompassIcon className="size-3.5 text-primary" />
          <span className="font-medium">{trace.taskFocus.title}</span>
        </button>
      )}

      <LiveTraceSegments
        sessionId={sessionId}
        segments={segments}
        questionSlots={questionSlots}
        onJumpToSection={onJumpToSection}
        messageById={messageById}
        attachmentMap={attachmentMap}
        showActiveDetails={detailsOpen}
      />
    </>
  );
}

function CompactionDivider({ summary }: { summary: string }) {
  return (
    <details
      data-slot="compaction-divider"
      className="group my-2 rounded-md border border-dashed border-border/70 bg-muted/30 px-3 py-2 text-[11px] text-muted-foreground"
    >
      <summary className="cursor-pointer list-none [&::-webkit-details-marker]:hidden">
        上下文已压缩
      </summary>
      <div className="mt-2 border-t pt-2 text-xs leading-5 whitespace-pre-wrap">
        {summary}
      </div>
    </details>
  );
}

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
  pendingQueue?: PendingQueueItem[];
  busyQueueIds?: ReadonlySet<string>;
  onEditPending?: (
    queueId: string,
    content: string,
    commentRefs: SessionCommentRef[],
  ) => Promise<void>;
  onRecallPending?: (item: PendingQueueItem) => Promise<void>;
  onDeletePending?: (queueId: string) => Promise<void>;
  onInterrupt?: () => Promise<void>;
  contentClassName?: string;
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
  pendingQueue = [],
  busyQueueIds,
  onEditPending,
  onRecallPending,
  onDeletePending,
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
  const messageById = useMemo(() => {
    const map = new Map<string, SessionMessage>();
    for (const message of messages) {
      map.set(message.message_id, message);
    }
    return map;
  }, [messages]);
  const inlineUserMessageIds = useMemo(() => {
    const ids = new Set<string>();
    for (const trace of traces) {
      for (const segment of trace.segments ?? []) {
        if (segment.kind === "user_message") {
          ids.add(segment.message_id);
        }
      }
    }
    for (const segment of liveMain?.segments ?? []) {
      if (segment.kind === "user_message") {
        ids.add(segment.messageId);
      }
    }
    return ids;
  }, [liveMain, traces]);
  const pendingEndRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (pendingQueue.length > 0) {
      pendingEndRef.current?.scrollIntoView({
        behavior: "smooth",
        block: "end",
      });
    }
  }, [pendingQueue.length]);

  const traceByRunId = useMemo(() => {
    const map = new Map<string, RunTraceSummary>();
    for (const trace of traces) {
      map.set(trace.run_id, trace);
    }
    return map;
  }, [traces]);

  const liveSegments = useMemo(
    () => (liveMain ? buildLiveSegments(liveMain, liveSubagents) : []),
    [liveMain, liveSubagents],
  );

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
        msg.role === "assistant" ||
        msg.role === "tool" ||
        msg.role === "compaction" ||
        (msg.role === "user" && !inlineUserMessageIds.has(msg.message_id)),
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
  }, [messages, questionsByRunId, traceByRunId, inlineUserMessageIds]);

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
      key={sessionId}
      autoScroll
      defaultScrollPosition="end"
      scrollEdgeThreshold={24}
    >
      <MessageScroller className="h-full">
        <MessageScrollerViewport className="px-4 data-[pending-scroll]:opacity-0">
          <MessageScrollerContent className={`${contentClassName} py-4`}>
            {visibleMessages.map((msg) => {
              const trace = msg.run_id
                ? traceByRunId.get(msg.run_id)
                : undefined;
              if (msg.role === "compaction") {
                return (
                  <MessageScrollerItem
                    key={msg.message_id}
                    messageId={msg.message_id}
                  >
                    <CompactionDivider summary={msg.content} />
                  </MessageScrollerItem>
                );
              }
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
                      messageById={messageById}
                      attachmentMap={attachmentMap}
                      onJumpToSection={onJumpToSection}
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
                    messageById={messageById}
                  />
                </MessageScrollerItem>
              );
            })}

            {liveMain && (
              <MessageScrollerItem messageId="live-main">
                <MessageGroup>
                  <Message align="start">
                    <MessageContent className="w-full gap-1">
                      <LiveRunSection
                        key={liveMain.runId ?? "live-main"}
                        sessionId={sessionId}
                        trace={liveMain}
                        subagents={liveSubagents}
                        segments={liveSegments}
                        questionSlots={questionSlots}
                        onJumpToSection={onJumpToSection}
                        onInterrupt={onInterrupt}
                        messageById={messageById}
                        attachmentMap={attachmentMap}
                      />
                    </MessageContent>
                  </Message>
                </MessageGroup>
              </MessageScrollerItem>
            )}

            {pendingQueue.map((item, index) => (
              <MessageScrollerItem
                key={`pending-${item.queue_id}`}
                messageId={`pending-${item.queue_id}`}
              >
                <PendingUserBubble
                  item={item}
                  index={index}
                  count={pendingQueue.length}
                  attachmentMap={attachmentMap}
                  isBusy={busyQueueIds?.has(item.queue_id) ?? false}
                  onJumpToSection={onJumpToSection}
                  onEdit={onEditPending}
                  onRecall={onRecallPending}
                  onDelete={onDeletePending}
                />
              </MessageScrollerItem>
            ))}
            {pendingQueue.length > 0 && <div ref={pendingEndRef} />}

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

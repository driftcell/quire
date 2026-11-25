import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  HistoricalRunTrace,
  LiveTraceSegments,
} from "@/components/chat/ExecutionTraceSection";
import { HistoricalMessageItem } from "@/components/chat/HistoricalMessageItem";
import { PendingUserBubble } from "@/components/chat/PendingUserBubble";
import { QuestionCard } from "@/components/chat/QuestionCard";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import {
  buildLiveRunStatusModel,
  buildLiveSegments,
  hasTraceActivity,
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
  onJumpToSection,
  showFinalText,
}: {
  sessionId: string;
  trace: LiveAgentTrace;
  subagents: LiveSubagentRun[];
  segments: ReturnType<typeof buildLiveSegments>;
  onJumpToSection?: (sectionId: string) => void;
  showFinalText: boolean;
}) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [fallbackStartedAt] = useState(() => Date.now());
  const [now, setNow] = useState(fallbackStartedAt);
  const startedAt = trace.startedAtMs ?? fallbackStartedAt;

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

  const elapsed = Math.max(0, Math.floor((now - startedAt) / 1000));
  const headerModel = buildLiveRunStatusModel(trace, subagents, elapsed);
  const showHeader =
    trace.isStreaming ||
    hasTraceActivity(segments) ||
    Boolean(trace.taskFocus) ||
    headerModel.milestones.length > 0;

  if (!showHeader && !showFinalText) {
    return null;
  }

  return (
    <div className="w-full space-y-2.5">
      {showHeader && (
        <RunActivityHeader
          model={headerModel}
          isStreaming={trace.isStreaming}
          detailsOpen={detailsOpen}
          onToggleDetails={() => setDetailsOpen((open) => !open)}
          onJumpToSection={onJumpToSection}
        />
      )}

      <LiveTraceSegments
        sessionId={sessionId}
        trace={trace}
        subagents={subagents}
        segments={segments}
        detailsOpen={detailsOpen}
        onOpenChange={setDetailsOpen}
        onJumpToSection={onJumpToSection}
        showFinalText={showFinalText}
      />
    </div>
  );
}

function CompactionDivider({ summary }: { summary: string }) {
  return (
    <details
      data-slot="compaction-divider"
      className="group my-1 rounded-lg border border-dashed border-border/70 bg-muted/25 px-3 py-2 text-[11px] text-muted-foreground"
    >
      <summary className="cursor-pointer list-none font-medium [&::-webkit-details-marker]:hidden">
        上下文已压缩
      </summary>
      <div className="mt-2 border-t border-border/50 pt-2 text-xs leading-5 whitespace-pre-wrap">
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

  const liveSegments = useMemo(
    () => (liveMain ? buildLiveSegments(liveMain, liveSubagents) : []),
    [liveMain, liveSubagents],
  );

  type QuestionCardEntry = {
    question: PendingQuestion;
    resolvedState?: {
      status: "answered" | "skipped" | "dismissed";
      answers?: QuestionAnswer[];
    };
  };

  const questionCardById = useMemo(() => {
    const byId = new Map<string, QuestionCardEntry>();
    for (const item of resolvedQuestions) {
      byId.set(item.question.question_id, {
        question: item.question,
        resolvedState: { status: item.status, answers: item.answers },
      });
    }
    if (pendingQuestion) {
      byId.set(pendingQuestion.question_id, { question: pendingQuestion });
    }
    return byId;
  }, [pendingQuestion, resolvedQuestions]);

  const answerQuestionRef = useRef(onAnswerQuestion);
  useEffect(() => {
    answerQuestionRef.current = onAnswerQuestion;
  }, [onAnswerQuestion]);

  const renderQuestionCard = useCallback(
    (entry: QuestionCardEntry) => (
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

  const visibleMessages = useMemo(
    () =>
      messages.filter(
        (msg) =>
          msg.role === "user" ||
          msg.role === "assistant" ||
          msg.role === "question" ||
          msg.role === "compaction",
      ),
    [messages],
  );

  const firstTraceAnchorForQuestion = useMemo(() => {
    const assistantRuns = new Set(
      visibleMessages
        .filter((msg) => msg.role === "assistant" && msg.run_id)
        .map((msg) => msg.run_id),
    );
    const anchors = new Set<string>();
    const claimedRuns = new Set<string>();
    for (const msg of visibleMessages) {
      if (
        msg.role === "question" &&
        msg.run_id &&
        !assistantRuns.has(msg.run_id) &&
        !claimedRuns.has(msg.run_id) &&
        traceByRunId.has(msg.run_id)
      ) {
        anchors.add(msg.message_id);
        claimedRuns.add(msg.run_id);
      }
    }
    return anchors;
  }, [traceByRunId, visibleMessages]);

  const lastUserMessageId = useMemo(() => {
    for (let index = visibleMessages.length - 1; index >= 0; index -= 1) {
      const message = visibleMessages[index];
      if (message?.role === "user") {
        return message.message_id;
      }
    }
    return null;
  }, [visibleMessages]);

  const persistedQuestionIds = useMemo(
    () =>
      new Set(
        visibleMessages
          .filter((msg) => msg.role === "question" && msg.question_id)
          .map((msg) => msg.question_id),
      ),
    [visibleMessages],
  );

  const unpersistedLiveQuestion =
    pendingQuestion && !persistedQuestionIds.has(pendingQuestion.question_id)
      ? pendingQuestion
      : null;

  const liveRunId = liveMain?.runId;
  const liveRunCompleted = liveRunId
    ? traceByRunId.get(liveRunId)?.status === "completed"
    : false;
  const showLiveMain = Boolean(
    liveMain &&
      (liveMain.isStreaming ||
        !liveRunId ||
        !(
          traceByRunId.has(liveRunId) &&
          visibleMessages.some(
            (msg) => msg.run_id === liveRunId && msg.role !== "user",
          )
        )),
  );
  const orphanTraces = useMemo(() => {
    const rendered = new Set<string>();
    for (const msg of visibleMessages) {
      if (msg.run_id && msg.role !== "user") {
        rendered.add(msg.run_id);
      }
    }
    if (showLiveMain && liveRunId) {
      rendered.add(liveRunId);
    }
    return traces.filter(
      (trace) =>
        !rendered.has(trace.run_id) &&
        ((trace.segments?.length ?? 0) > 0 || Boolean(trace.task_focus)),
    );
  }, [liveRunId, showLiveMain, traces, visibleMessages]);

  type TimelineEntry =
    | { kind: "message"; msg: SessionMessage }
    | { kind: "orphan-trace"; trace: RunTraceSummary };

  const timelineEntries = useMemo<TimelineEntry[]>(() => {
    const timestamped: Array<{ trace: RunTraceSummary; startedAtMs: number; order: number }> = [];
    const untimestamped: RunTraceSummary[] = [];
    orphanTraces.forEach((trace, order) => {
      const parsed = trace.started_at ? Date.parse(trace.started_at) : NaN;
      if (Number.isFinite(parsed)) {
        timestamped.push({ trace, startedAtMs: parsed, order });
      } else {
        untimestamped.push(trace);
      }
    });
    timestamped.sort((a, b) => a.startedAtMs - b.startedAtMs || a.order - b.order);

    const entries: TimelineEntry[] = [];
    let orphanCursor = 0;
    for (const msg of visibleMessages) {
      const msgTime = Date.parse(msg.created_at);
      if (Number.isFinite(msgTime)) {
        while (orphanCursor < timestamped.length) {
          const nextOrphan = timestamped[orphanCursor];
          if (!nextOrphan) break;
          const precedes =
            msg.role === "user"
              ? nextOrphan.startedAtMs < msgTime
              : nextOrphan.startedAtMs <= msgTime;
          if (!precedes) break;
          entries.push({ kind: "orphan-trace", trace: nextOrphan.trace });
          orphanCursor += 1;
        }
      }
      entries.push({ kind: "message", msg });
    }
    while (orphanCursor < timestamped.length) {
      const nextOrphan = timestamped[orphanCursor];
      if (nextOrphan) {
        entries.push({ kind: "orphan-trace", trace: nextOrphan.trace });
      }
      orphanCursor += 1;
    }
    for (const trace of untimestamped) {
      entries.push({ kind: "orphan-trace", trace });
    }
    return entries;
  }, [orphanTraces, visibleMessages]);

  return (
    <MessageScrollerProvider
      key={sessionId}
      autoScroll
      defaultScrollPosition="end"
      scrollEdgeThreshold={24}
    >
      <MessageScroller className="h-full">
        <MessageScrollerViewport className="px-4 data-[pending-scroll]:opacity-0">
          <MessageScrollerContent className={`${contentClassName} gap-6 py-6`}>
            {timelineEntries.map((item) => {
              if (item.kind === "orphan-trace") {
                const trace = item.trace;
                return (
                  <MessageScrollerItem
                    key={`orphan-${trace.run_id}`}
                    messageId={`orphan-${trace.run_id}`}
                  >
                    <Message align="start" className="items-start">
                      <MessageContent className="min-w-0 w-full flex-1">
                        <HistoricalRunTrace
                          sessionId={sessionId}
                          trace={trace}
                          onJumpToSection={onJumpToSection}
                        />
                      </MessageContent>
                    </Message>
                  </MessageScrollerItem>
                );
              }
              const msg = item.msg;
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
              if (msg.role === "question") {
                const entry = msg.question_id
                  ? questionCardById.get(msg.question_id)
                  : undefined;
                const showQuestionTrace =
                  Boolean(trace) &&
                  firstTraceAnchorForQuestion.has(msg.message_id);
                if (!entry && !showQuestionTrace) {
                  return null;
                }
                return (
                  <MessageScrollerItem
                    key={msg.message_id}
                    messageId={msg.message_id}
                  >
                    <Message align="start" className="items-start">
                      <MessageContent className="min-w-0 w-full flex-1 gap-3">
                        {showQuestionTrace && trace && (
                          <HistoricalRunTrace
                            sessionId={sessionId}
                            trace={trace}
                            onJumpToSection={onJumpToSection}
                          />
                        )}
                        {entry && renderQuestionCard(entry)}
                      </MessageContent>
                    </Message>
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
                    onJumpToSection={onJumpToSection}
                  />
                </MessageScrollerItem>
              );
            })}

            {((showLiveMain && liveMain) || unpersistedLiveQuestion) && (
              <MessageScrollerItem messageId="live-main">
                <MessageGroup>
                  <Message align="start" className="items-start">
                    <MessageContent className="min-w-0 w-full flex-1 gap-3">
                      {showLiveMain && liveMain && (
                        <LiveRunSection
                          key={liveMain.runId ?? "live-main"}
                          sessionId={sessionId}
                          trace={liveMain}
                          subagents={liveSubagents}
                          segments={liveSegments}
                          onJumpToSection={onJumpToSection}
                          showFinalText={liveRunCompleted}
                        />
                      )}
                      {unpersistedLiveQuestion &&
                        renderQuestionCard({ question: unpersistedLiveQuestion })}
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
          </MessageScrollerContent>
        </MessageScrollerViewport>
        <MessageScrollerButton />
      </MessageScroller>
    </MessageScrollerProvider>
  );
}

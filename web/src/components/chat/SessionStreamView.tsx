import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { AgentAvatar } from "@/components/chat/agentAvatar";
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
}: {
  sessionId: string;
  trace: LiveAgentTrace;
  subagents: LiveSubagentRun[];
  segments: ReturnType<typeof buildLiveSegments>;
  questionSlots: Map<string, ReactNode>;
  onJumpToSection?: (sectionId: string) => void;
  onInterrupt?: () => Promise<void>;
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

  return (
    <div className="w-full space-y-2.5">
      <RunActivityHeader
        model={headerModel}
        isStreaming={trace.isStreaming}
        detailsOpen={detailsOpen}
        onToggleDetails={() => setDetailsOpen((open) => !open)}
        onJumpToSection={onJumpToSection}
        onInterrupt={onInterrupt}
      />

      <LiveTraceSegments
        sessionId={sessionId}
        segments={segments}
        questionSlots={questionSlots}
        detailsOpen={detailsOpen}
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
        msg.role === "user",
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
          <MessageScrollerContent className={`${contentClassName} gap-5 py-5`}>
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
                    <Message align="start" className="items-start gap-2.5">
                      <AgentAvatar actor="main" size="sm" className="mt-0.5" />
                      <MessageContent className="min-w-0 flex-1">
                        <HistoricalRunTrace
                          sessionId={sessionId}
                          trace={trace}
                          questionSlots={questionSlots}
                          onJumpToSection={onJumpToSection}
                        />
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
                    questionSlots={questionSlots}
                    onJumpToSection={onJumpToSection}
                  />
                </MessageScrollerItem>
              );
            })}

            {liveMain &&
              (liveMain.isStreaming ||
                !liveMain.runId ||
                !(
                  traceByRunId.has(liveMain.runId) &&
                  visibleMessages.some(
                    (msg) =>
                      msg.run_id === liveMain.runId && msg.role !== "user",
                  )
                )) && (
                <MessageScrollerItem messageId="live-main">
                  <MessageGroup>
                    <Message align="start" className="items-start gap-2.5">
                      <AgentAvatar
                        actor="main"
                        size="sm"
                        active={liveMain.isStreaming}
                        className="mt-1"
                      />
                      <MessageContent className="min-w-0 flex-1 gap-2">
                        <LiveRunSection
                          key={liveMain.runId ?? "live-main"}
                          sessionId={sessionId}
                          trace={liveMain}
                          subagents={liveSubagents}
                          segments={liveSegments}
                          questionSlots={questionSlots}
                          onJumpToSection={onJumpToSection}
                          onInterrupt={onInterrupt}
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

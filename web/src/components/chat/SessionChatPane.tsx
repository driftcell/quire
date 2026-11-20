import { SessionStreamView } from "@/components/chat/SessionStreamView";
import {
  PromptComposer,
  type QuotedChatSnippet,
} from "@/components/chat/PromptComposer";
import { Skeleton } from "@/components/ui/skeleton";
import type {
  ActiveRunStatus,
  AttachmentMeta,
  ContextUsage,
  PendingQueueItem,
  PendingQuestion,
  QuestionAnswer,
  RunTraceSummary,
  SessionMessage,
} from "@/contracts/session-protocol";
import type { ResolvedQuestionEntry } from "@/hooks/useSessionEventStream";
import type { CommentRefPayload } from "@/lib/commentRefXml";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

export interface SessionChatPaneProps {
  sessionId: string;
  isLoading: boolean;
  messages: SessionMessage[];
  attachments: AttachmentMeta[];
  traces: RunTraceSummary[];
  hasReportData: boolean;
  liveMain: LiveAgentTrace | null;
  liveSubagents: LiveSubagentRun[];
  pendingQuestion: PendingQuestion | null;
  resolvedQuestions: ResolvedQuestionEntry[];
  isAnsweringQuestion: boolean;
  onAnswerQuestion: (
    questionId: string,
    answers: QuestionAnswer[],
    skipped: boolean,
  ) => void;
  onJumpToSection: (sectionId: string) => void;
  pendingQueue: PendingQueueItem[];
  busyQueueIds?: ReadonlySet<string>;
  onEditPending: (queueId: string, content: string) => Promise<void>;
  onRecallPending: (item: PendingQueueItem) => Promise<void>;
  onDeletePending: (queueId: string) => Promise<void>;
  onInterrupt: () => Promise<void>;
  runStatus: ActiveRunStatus;
  compacting?: boolean;
  contextUsage?: ContextUsage;
  quotedSnippet: QuotedChatSnippet | null;
  pendingCommentRefs: CommentRefPayload[];
  prefill: { key: string; text: string } | null;
  onClearQuote: () => void;
  onRemoveCommentRef: (threadId: string) => void;
  onClearCommentRefs: () => void;
  onSend: (text: string, files: File[]) => Promise<void>;
  onRetry: () => Promise<void>;
  onCompact: () => Promise<void>;
}

export function SessionChatPane({
  sessionId,
  isLoading,
  messages,
  attachments,
  traces,
  hasReportData,
  liveMain,
  liveSubagents,
  pendingQuestion,
  resolvedQuestions,
  isAnsweringQuestion,
  onAnswerQuestion,
  onJumpToSection,
  pendingQueue,
  busyQueueIds,
  onEditPending,
  onRecallPending,
  onDeletePending,
  onInterrupt,
  runStatus,
  compacting = false,
  contextUsage,
  quotedSnippet,
  pendingCommentRefs,
  prefill,
  onClearQuote,
  onRemoveCommentRef,
  onClearCommentRefs,
  onSend,
  onRetry,
  onCompact,
}: SessionChatPaneProps) {
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden bg-background">
      <div className="min-h-0 flex-1">
        {isLoading ? (
          <div className="h-full overflow-y-auto px-4">
            <div className="mx-auto max-w-3xl space-y-4 py-6">
              <Skeleton className="ml-auto h-10 w-2/3 rounded-2xl" />
              <Skeleton className="h-24 w-5/6 rounded-2xl" />
              <Skeleton className="h-16 w-3/4 rounded-2xl" />
            </div>
          </div>
        ) : (
          <SessionStreamView
            sessionId={sessionId}
            messages={messages}
            contentClassName={
              hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"
            }
            onInterrupt={onInterrupt}
            attachments={attachments}
            traces={traces}
            liveMain={liveMain}
            liveSubagents={liveSubagents}
            pendingQuestion={pendingQuestion}
            resolvedQuestions={resolvedQuestions}
            isAnsweringQuestion={isAnsweringQuestion}
            onAnswerQuestion={onAnswerQuestion}
            onJumpToSection={onJumpToSection}
            pendingQueue={pendingQueue}
            busyQueueIds={busyQueueIds}
            onEditPending={onEditPending}
            onRecallPending={onRecallPending}
            onDeletePending={onDeletePending}
          />
        )}
      </div>

      <div
        className={
          hasReportData
            ? "shrink-0 bg-background/95 px-3 pt-1 pb-3"
            : "shrink-0 bg-background/95 px-4 pt-1 pb-5"
        }
      >
        <div
          className={hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"}
        >
          <PromptComposer
            runStatus={runStatus}
            compacting={compacting}
            contextUsage={contextUsage}
            quotedSnippet={quotedSnippet}
            pendingCommentRefs={pendingCommentRefs}
            prefill={prefill}
            onClearQuote={onClearQuote}
            onRemoveCommentRef={onRemoveCommentRef}
            onClearCommentRefs={onClearCommentRefs}
            onSend={onSend}
            onRetry={onRetry}
            onCompact={onCompact}
          />
        </div>
      </div>
    </div>
  );
}

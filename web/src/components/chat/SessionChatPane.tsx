import { useMemo } from "react";
import { TerminalIcon } from "lucide-react";
import {
  buildChatRichContext,
  ChatRichContext,
} from "@/components/chat/chatRichContext";
import { SessionStreamView } from "@/components/chat/SessionStreamView";
import type { DocumentResponse } from "@/lib/document";
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
  SessionCommentRef,
  SessionMessage,
} from "@/contracts/session-protocol";
import type { ResolvedQuestionEntry } from "@/hooks/useSessionEventStream";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

export interface SessionChatPaneProps {
  sessionId: string;
  isLoading: boolean;
  messages: SessionMessage[];
  attachments: AttachmentMeta[];
  traces: RunTraceSummary[];
  hasReportData: boolean;
  createdVia?: string;
  documentView?: DocumentResponse | null;
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
  onEditPending: (
    queueId: string,
    content: string,
    commentRefs: SessionCommentRef[],
  ) => Promise<void>;
  onRecallPending: (item: PendingQueueItem) => Promise<void>;
  onDeletePending: (queueId: string) => Promise<void>;
  onInterrupt: () => Promise<void>;
  runStatus: ActiveRunStatus;
  compacting?: boolean;
  contextUsage?: ContextUsage;
  streamingEstimatedOutputTokens?: number;
  quotedSnippet: QuotedChatSnippet | null;
  pendingCommentRefs: SessionCommentRef[];
  prefill: { key: string; text: string } | null;
  onClearQuote: () => void;
  onRemoveCommentRef: (threadId: string) => void;
  onClearCommentRefs: () => void;
  onSend: (
    text: string,
    files: File[],
    commentRefs: SessionCommentRef[],
  ) => Promise<void>;
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
  createdVia,
  documentView,
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
  streamingEstimatedOutputTokens = 0,
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
  const richContext = useMemo(
    () => buildChatRichContext(documentView, traces, liveMain, liveSubagents),
    [documentView, traces, liveMain, liveSubagents],
  );

  const showEmptyExternalNotice =
    !isLoading &&
    hasReportData &&
    messages.length === 0 &&
    traces.length === 0 &&
    !liveMain &&
    (createdVia === "mcp" || true);

  return (
    <ChatRichContext.Provider value={richContext}>
      <div className="flex h-full min-w-0 flex-col overflow-hidden bg-background">
        <div className="min-h-0 flex-1">
          {isLoading ? (
            <div className="no-scrollbar h-full overflow-y-auto px-4">
              <div className="mx-auto max-w-3xl space-y-4 py-6">
                <Skeleton className="ml-auto h-10 w-2/3 rounded-2xl" />
                <Skeleton className="h-24 w-5/6 rounded-2xl" />
                <Skeleton className="h-16 w-3/4 rounded-2xl" />
              </div>
            </div>
          ) : showEmptyExternalNotice ? (
            <div className="flex h-full flex-col justify-center px-5 py-8">
              <div className="mx-auto w-full max-w-sm border-l-2 border-[#1D4ED8] pl-3.5 text-left">
                <div className="flex items-center gap-1.5 text-xs font-medium text-foreground">
                  <TerminalIcon
                    className="size-3.5 shrink-0 text-[#1D4ED8]"
                    aria-hidden="true"
                  />
                  <span>外部代理报告已绑定当前会话</span>
                </div>
                <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                  左侧报告由外部代理通过{" "}
                  <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">
                    /mcp
                  </code>{" "}
                  创建并实时同步。你可以仅在左侧审阅报告、添加段落批注或导出 A4
                  PDF，无需使用此对话栏；也可以随时在下方发送指令，交由内置代理继续润色或改写。
                </p>
              </div>
            </div>
          ) : (
            <SessionStreamView
              sessionId={sessionId}
              messages={messages}
              contentClassName={
                hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"
              }
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
              ? "shrink-0 border-t border-border/40 bg-gradient-to-t from-background via-background to-background/85 px-3 pt-2 pb-3"
              : "shrink-0 border-t border-border/40 bg-gradient-to-t from-background via-background to-background/85 px-4 pt-2 pb-5"
          }
        >
          <div
            className={
              hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"
            }
          >
            <PromptComposer
              runStatus={runStatus}
              compacting={compacting}
              contextUsage={contextUsage}
              streamingEstimatedOutputTokens={streamingEstimatedOutputTokens}
              quotedSnippet={quotedSnippet}
              pendingCommentRefs={pendingCommentRefs}
              prefill={prefill}
              onClearQuote={onClearQuote}
              onRemoveCommentRef={onRemoveCommentRef}
              onClearCommentRefs={onClearCommentRefs}
              onSend={onSend}
              onInterrupt={onInterrupt}
              onRetry={onRetry}
              onCompact={onCompact}
            />
          </div>
        </div>
      </div>
    </ChatRichContext.Provider>
  );
}

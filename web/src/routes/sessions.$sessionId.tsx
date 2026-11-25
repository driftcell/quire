import { useCallback, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { getDocument, listComments } from "@/api/quire";
import {
  compactSession,
  createSessionComment,
  getSession,
  interruptSession,
  listSessionTraces,
  replySessionComment,
  retrySession,
  updateSessionCommentStatus,
} from "@/api/runtime";
import type { QuotedChatSnippet } from "@/components/chat/PromptComposer";
import { SessionChatPane } from "@/components/chat/SessionChatPane";
import { SessionCommentsPane } from "@/components/report/SessionCommentsPane";
import {
  FloatingCommentPopover,
  type FloatingCommentAnchor,
} from "@/components/report/FloatingCommentPopover";
import { SessionReportPane } from "@/components/report/SessionReportPane";
import {
  commentThreadBlockIds,
  type ReportSelectionDescriptor,
} from "@/components/report/reportSelection";
import { SessionRightPane } from "@/components/report/SessionRightPane";
import { Skeleton } from "@/components/ui/skeleton";
import {
  SessionWorkspaceLayout,
  type SessionMobileTab,
} from "@/components/report/SessionWorkspaceLayout";
import { flashHighlightElement } from "@/components/report/reportDom";
import { useSidebar } from "@/components/ui/sidebar";
import type { CommentThread } from "@/contracts/report-protocol";
import type {
  ActiveRunStatus,
  ContextUsage,
} from "@/contracts/session-protocol";
import { useDocumentEventStream } from "@/hooks/useDocumentEventStream";
import { useSessionEventStream } from "@/hooks/useSessionEventStream";
import { useSessionMessageActions } from "@/hooks/useSessionMessageActions";
import { useSessionQueueActions } from "@/hooks/useSessionQueueActions";
import { useIsMobile } from "@/hooks/use-mobile";
import { useWorkspacePreferences } from "@/hooks/useWorkspacePreferences";

export const Route = createFileRoute("/sessions/$sessionId")({
  component: SessionWorkspaceRoute,
});

function SessionWorkspaceRoute() {
  const { sessionId } = Route.useParams();
  return <SessionWorkspace key={sessionId} sessionId={sessionId} />;
}

function SessionWorkspace({ sessionId }: { sessionId: string }) {
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const { open: leftSidebarOpen, toggleSidebar } = useSidebar();

  const { prefs, updatePrefs, toggleRightSidebar } = useWorkspacePreferences();

  const sessionQuery = useQuery({
    queryKey: ["session", sessionId],
    queryFn: () => getSession(sessionId),
    refetchOnMount: "always",
  });

  const tracesQuery = useQuery({
    queryKey: ["session-traces", sessionId],
    queryFn: () => listSessionTraces(sessionId),
  });

  const {
    liveDocId,
    liveRunStatus,
    liveUsage,
    streamingEstimatedOutputTokens,
    liveMain,
    liveSubagents,
    effectivePendingQuestion,
    mergedResolvedQuestions,
    recordResolvedQuestion,
    clearLivePendingQuestion,
  } = useSessionEventStream({
    sessionId,
    enabled: sessionQuery.isSuccess,
    lastEventSeq: sessionQuery.data?.last_event_seq ?? 0,
    serverActiveRunStatus: sessionQuery.data?.active_run_status,
    activeRunTrace: sessionQuery.data?.active_run_trace,
    activeSubagentTraces: sessionQuery.data?.active_subagent_traces,
    serverPendingQuestion: sessionQuery.data?.pending_question ?? null,
    serverResolvedQuestions: sessionQuery.data?.resolved_questions,
  });

  const [rightTab, setRightTab] = useState<"chat" | "comments">("chat");
  const [activeCommentThreadId, setActiveCommentThreadId] = useState<
    string | null
  >(null);
  const [floatingComment, setFloatingComment] =
    useState<FloatingCommentAnchor | null>(null);
  const [activeReportSelection, setActiveReportSelection] =
    useState<ReportSelectionDescriptor | null>(null);
  const [mobileTab, setMobileTab] = useState<SessionMobileTab>("report");
  const [quotedForChat, setQuotedForChat] = useState<QuotedChatSnippet | null>(
    null,
  );

  const effectiveDocId =
    liveDocId ??
    sessionQuery.data?.doc_id ??
    sessionQuery.data?.linked_doc_id ??
    null;
  const pendingQueue = useMemo(
    () => sessionQuery.data?.pending_queue ?? [],
    [sessionQuery.data?.pending_queue],
  );
  const pendingThreadIds = useMemo(() => {
    const ids = new Set<string>();
    for (const item of pendingQueue) {
      if (item.thread_id) {
        ids.add(item.thread_id);
      }
    }
    return ids;
  }, [pendingQueue]);

  const effectiveRunStatus: ActiveRunStatus =
    liveRunStatus ?? sessionQuery.data?.active_run_status ?? "idle";
  const effectiveUsage: ContextUsage | undefined =
    liveUsage ?? sessionQuery.data?.context_usage;

  const docQuery = useQuery({
    queryKey: ["doc", effectiveDocId],
    queryFn: () => {
      if (!effectiveDocId) {
        return Promise.reject(new Error("No document linked to this session"));
      }
      return getDocument(effectiveDocId);
    },
    enabled: Boolean(effectiveDocId),
  });

  const commentsQuery = useQuery({
    queryKey: ["comments", effectiveDocId],
    queryFn: () => {
      if (!effectiveDocId) {
        return Promise.reject(new Error("No document linked to this session"));
      }
      return listComments(effectiveDocId, "all");
    },
    enabled: Boolean(effectiveDocId),
  });

  const commentThreads = useMemo(
    () => commentsQuery.data ?? [],
    [commentsQuery.data],
  );

  const { ephemeralPreviews, activeActorsBySection } = useDocumentEventStream({
    docId: effectiveDocId,
    enabled: Boolean(effectiveDocId),
    currentRev: docQuery.data?.current_rev ?? 0,
    isDocumentReady: docQuery.isSuccess,
    isRunActive: effectiveRunStatus === "running",
  });

  const isEmptyContainerDoc =
    docQuery.data !== undefined &&
    docQuery.data.document.blocks.length === 0 &&
    docQuery.data.document.title === "会话研究资料与图表";

  // A linked document is enough to select the report workspace unless it is a
  // legacy 0-block chat container.
  const hasReportData =
    (effectiveDocId !== null && !isEmptyContainerDoc) || sessionQuery.isLoading;

  const jumpToSection = useCallback((sectionId: string) => {
    const el = window.document.getElementById(`block-${sectionId}`);
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    flashHighlightElement(el);
  }, []);

  const jumpToSectionFromChat = useCallback(
    (sectionId: string) => {
      if (isMobile) {
        setMobileTab("report");
      }
      jumpToSection(sectionId);
    },
    [isMobile, jumpToSection],
  );

  const showComments = useCallback(() => {
    setRightTab("comments");
    if (isMobile) {
      setMobileTab("comments");
    } else if (!prefs.rightSidebarOpen) {
      updatePrefs({ rightSidebarOpen: true });
    }
  }, [isMobile, prefs.rightSidebarOpen, updatePrefs]);

  const showChat = useCallback(() => {
    setRightTab("chat");
    if (isMobile) {
      setMobileTab("chat");
    } else if (!prefs.rightSidebarOpen) {
      updatePrefs({ rightSidebarOpen: true });
    }
  }, [isMobile, prefs.rightSidebarOpen, updatePrefs]);

  const handleViewModeChange = useCallback(
    (mode: typeof prefs.reportViewMode) => {
      updatePrefs({ reportViewMode: mode });
    },
    [updatePrefs],
  );
  const handleOpenCommentForBlock = useCallback(
    (anchor: FloatingCommentAnchor) => {
      setFloatingComment(anchor);
    },
    [],
  );
  const handleSelectCommentThread = useCallback(
    (thread: CommentThread) => {
      setActiveCommentThreadId(thread.thread_id);
      showComments();
    },
    [showComments],
  );

  const { isAnsweringQuestion, handleSend, handleAnswerQuestion } =
    useSessionMessageActions({
      sessionId,
      effectivePendingQuestion,
      recordResolvedQuestion,
      clearLivePendingQuestion,
    });

  const {
    pendingCommentRefs,
    setPendingCommentRefs,
    composerPrefill,
    busyQueueIds,
    handleUpdateQueueItem,
    handleRecallQueueItem,
    handleDeletePending,
  } = useSessionQueueActions({
    sessionId,
    effectiveDocId,
    pendingQueue,
    showChat,
  });

  const handleAskAgentWithQuote = useCallback(
    (selection: ReportSelectionDescriptor) => {
      setQuotedForChat({
        blockId: selection.blockId,
        endBlockId: selection.endBlockId,
        blockIds: selection.blockIds,
        exact: selection.anchorSnapshot,
      });
      showChat();
    },
    [showChat],
  );

  // Only open (unresolved) threads count as active report comments.
  const openCommentsCount = commentThreads.filter(
    (comment) => comment.status === "open",
  ).length;

  const chatPane = (
    <SessionChatPane
      sessionId={sessionId}
      isLoading={sessionQuery.isLoading}
      messages={sessionQuery.data?.messages ?? []}
      attachments={sessionQuery.data?.attachments ?? []}
      traces={tracesQuery.data ?? []}
      hasReportData={hasReportData}
      documentView={docQuery.data}
      liveMain={liveMain}
      liveSubagents={liveSubagents}
      pendingQuestion={effectivePendingQuestion}
      resolvedQuestions={mergedResolvedQuestions}
      isAnsweringQuestion={isAnsweringQuestion}
      onAnswerQuestion={(qId, answers, skipped) => {
        void handleAnswerQuestion(qId, answers, skipped);
      }}
      onJumpToSection={jumpToSectionFromChat}
      pendingQueue={pendingQueue}
      busyQueueIds={busyQueueIds}
      onEditPending={handleUpdateQueueItem}
      onRecallPending={handleRecallQueueItem}
      onDeletePending={handleDeletePending}
      onInterrupt={async () => {
        await interruptSession(sessionId);
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
      }}
      runStatus={effectiveRunStatus}
      compacting={sessionQuery.data?.compacting ?? false}
      contextUsage={effectiveUsage}
      streamingEstimatedOutputTokens={streamingEstimatedOutputTokens}
      quotedSnippet={quotedForChat}
      pendingCommentRefs={pendingCommentRefs}
      prefill={composerPrefill}
      onClearQuote={() => {
        setQuotedForChat(null);
        setActiveReportSelection(null);
      }}
      onRemoveCommentRef={(threadId) => {
        setPendingCommentRefs((prev) =>
          prev.filter((ref) => ref.thread_id !== threadId),
        );
      }}
      onClearCommentRefs={() => setPendingCommentRefs([])}
      onSend={handleSend}
      onRetry={async () => {
        await retrySession(sessionId);
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
      }}
      onCompact={async () => {
        const detail = await compactSession(sessionId);
        queryClient.setQueryData(["session", sessionId], detail);
      }}
    />
  );

  const commentsPane = effectiveDocId ? (
    <SessionCommentsPane
      sessionId={sessionId}
      docId={effectiveDocId}
      threads={commentsQuery.data ?? []}
      activeThreadId={activeCommentThreadId}
      pendingThreadIds={pendingThreadIds}
      isMobile={isMobile}
      onClose={() => updatePrefs({ rightSidebarOpen: false })}
      onSelectThread={(thread) => {
        setActiveCommentThreadId(thread.thread_id);
        jumpToSection(thread.block_id);
      }}
      onSelectBlock={jumpToSection}
      onArchived={(threadId) => {
        setPendingCommentRefs((prev) =>
          prev.filter((ref) => ref.thread_id !== threadId),
        );
        setActiveCommentThreadId((current) =>
          current === threadId ? null : current,
        );
      }}
      onDeleted={(thread) => {
        setPendingCommentRefs((prev) =>
          prev.filter((ref) => ref.thread_id !== thread.thread_id),
        );
        setActiveCommentThreadId((current) =>
          current === thread.thread_id ? null : current,
        );
      }}
    />
  ) : null;

  const reportLoadingPane = (
    <output
      data-slot="session-report-loading"
      aria-label="报告加载中"
      className="block h-full rounded-xl border bg-background px-5 py-8 shadow-2xs sm:px-10"
    >
      <div className="mx-auto max-w-[680px] space-y-4">
        <Skeleton className="h-9 w-3/5" />
        <Skeleton className="h-4 w-4/5" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-2/3" />
      </div>
    </output>
  );

  const reportPane = effectiveDocId ? (
    docQuery.data ? (
      <SessionReportPane
        docId={effectiveDocId}
        document={docQuery.data.document}
        computed={docQuery.data.computed}
        currentRev={docQuery.data.current_rev}
        viewMode={prefs.reportViewMode}
        ephemeralPreviews={ephemeralPreviews}
        comments={commentThreads}
        activeThreadId={activeCommentThreadId}
        activeActorsBySection={activeActorsBySection}
        openCommentsCount={openCommentsCount}
        isMobile={isMobile}
        rightSidebarOpen={prefs.rightSidebarOpen}
        onToggleRightSidebar={toggleRightSidebar}
        onViewModeChange={handleViewModeChange}
        onJumpToSection={jumpToSection}
        onShowComments={showComments}
        onOpenCommentForBlock={handleOpenCommentForBlock}
        onSelectCommentThread={handleSelectCommentThread}
        onAskAgentWithQuote={handleAskAgentWithQuote}
        activeSelection={activeReportSelection}
        onActiveSelectionChange={setActiveReportSelection}
      />
    ) : docQuery.error ? (
      <div
        role="alert"
        className="flex h-full items-center justify-center rounded-xl border bg-background text-sm text-destructive"
      >
        报告加载失败：{docQuery.error.message}
      </div>
    ) : (
      reportLoadingPane
    )
  ) : sessionQuery.isLoading ? (
    reportLoadingPane
  ) : null;

  const rightPane = (
    <SessionRightPane
      openCommentsCount={openCommentsCount}
      activeTab={rightTab}
      onTabChange={setRightTab}
      chatPane={chatPane}
      commentsPane={commentsPane}
    />
  );

  const floatingBlockThreads = useMemo(() => {
    if (!floatingComment) return [];
    return (commentsQuery.data ?? []).filter(
      (thread) =>
        thread.status === "open" &&
        commentThreadBlockIds(thread).includes(floatingComment.blockId),
    );
  }, [commentsQuery.data, floatingComment]);

  return (
    <>
      <SessionWorkspaceLayout
        isMobile={isMobile}
        hasReportData={hasReportData}
        sessionTitle={sessionQuery.data?.title ?? "加载中…"}
        leftSidebarOpen={leftSidebarOpen}
        toggleSidebar={toggleSidebar}
        rightSidebarOpen={prefs.rightSidebarOpen}
        rightPanelPct={prefs.rightPanelPct}
        reportPane={reportPane}
        chatPane={chatPane}
        rightPane={rightPane}
        commentsPane={commentsPane}
        mobileTab={mobileTab}
        onMobileTabChange={setMobileTab}
      />

      {effectiveDocId && (
        <FloatingCommentPopover
          anchor={floatingComment}
          existingCount={floatingBlockThreads.length}
          existingThreads={floatingBlockThreads}
          onClose={() => {
            setFloatingComment(null);
            setActiveReportSelection(null);
          }}
          onAddComment={async (anchor, body) => {
            const thread = await createSessionComment(sessionId, {
              block_id: anchor.blockId,
              end_block_id: anchor.endBlockId,
              anchor_blocks: anchor.blockIds,
              anchor_snapshot: anchor.anchorSnapshot,
              body,
              quote: anchor.quote,
              end_quote: anchor.endQuote,
            });
            void queryClient.invalidateQueries({
              queryKey: ["comments", effectiveDocId],
            });
            setActiveCommentThreadId(thread.thread_id);
            showComments();
          }}
          onReplyThread={async (threadId, body) => {
            await replySessionComment(sessionId, threadId, { body });
            void queryClient.invalidateQueries({
              queryKey: ["comments", effectiveDocId],
            });
          }}
          onResolveThread={async (threadId) => {
            await updateSessionCommentStatus(sessionId, threadId, "archived");
            void queryClient.invalidateQueries({
              queryKey: ["comments", effectiveDocId],
            });
            setPendingCommentRefs((prev) =>
              prev.filter((ref) => ref.thread_id !== threadId),
            );
            setActiveCommentThreadId((current) =>
              current === threadId ? null : current,
            );
          }}
          onSelectThread={(thread) => {
            setActiveCommentThreadId(thread.thread_id);
            showComments();
          }}
        />
      )}
    </>
  );
}

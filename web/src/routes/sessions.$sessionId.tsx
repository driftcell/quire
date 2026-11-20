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
  retrySession,
} from "@/api/runtime";
import type { QuotedChatSnippet } from "@/components/chat/PromptComposer";
import { SessionChatPane } from "@/components/chat/SessionChatPane";
import { SessionCommentsPane } from "@/components/report/SessionCommentsPane";
import {
  FloatingCommentPopover,
  type FloatingCommentAnchor,
} from "@/components/report/FloatingCommentPopover";
import { SessionReportPane } from "@/components/report/SessionReportPane";
import { SessionRightPane } from "@/components/report/SessionRightPane";
import {
  SessionWorkspaceLayout,
  type SessionMobileTab,
} from "@/components/report/SessionWorkspaceLayout";
import { flashHighlightElement } from "@/components/report/reportDom";
import { useSidebar } from "@/components/ui/sidebar";
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

  const {
    prefs,
    updatePrefs,
    rightPanelAnimating,
    rightPanelRef,
    toggleRightSidebar,
  } = useWorkspacePreferences();

  const sessionQuery = useQuery({
    queryKey: ["session", sessionId],
    queryFn: () => getSession(sessionId),
  });

  const tracesQuery = useQuery({
    queryKey: ["session-traces", sessionId],
    queryFn: () => listSessionTraces(sessionId),
  });

  const {
    liveDocId,
    liveRunStatus,
    liveUsage,
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
    serverPendingQuestion: sessionQuery.data?.pending_question ?? null,
    serverResolvedQuestions: sessionQuery.data?.resolved_questions,
  });

  const [rightTab, setRightTab] = useState<"chat" | "comments">("chat");
  const [activeCommentThreadId, setActiveCommentThreadId] = useState<
    string | null
  >(null);
  const [floatingComment, setFloatingComment] =
    useState<FloatingCommentAnchor | null>(null);
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

  const { ephemeralPreviews, activeActorsBySection } = useDocumentEventStream({
    docId: effectiveDocId,
    enabled: docQuery.isSuccess,
    currentRev: docQuery.data?.current_rev ?? 0,
  });

  const blockCount = docQuery.data?.document.blocks.length ?? 0;
  const hasReportData = useMemo(() => {
    if (!effectiveDocId) return false;
    return blockCount > 0 || ephemeralPreviews.size > 0;
  }, [effectiveDocId, blockCount, ephemeralPreviews.size]);

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
    (blockId: string, exact: string) => {
      setQuotedForChat({ blockId, exact });
      showChat();
    },
    [showChat],
  );

  const openCommentThread = useCallback(
    (threadId: string, blockId: string) => {
      setActiveCommentThreadId(threadId);
      showComments();
      jumpToSection(blockId);
    },
    [jumpToSection, showComments],
  );

  const openCommentsCount =
    commentsQuery.data?.filter((c) => c.status !== "archived").length ?? 0;

  const handleChatPanelResize = useCallback(
    (pct: number) => {
      if (pct <= 1 && prefs.rightSidebarOpen) {
        updatePrefs({ rightSidebarOpen: false });
        return;
      }
      if (prefs.rightSidebarOpen && pct >= 20 && pct <= 60) {
        updatePrefs({ rightPanelPct: Math.round(pct) });
      }
    },
    [prefs.rightSidebarOpen, updatePrefs],
  );

  const chatPane = (
    <SessionChatPane
      sessionId={sessionId}
      isLoading={sessionQuery.isLoading}
      messages={sessionQuery.data?.messages ?? []}
      attachments={sessionQuery.data?.attachments ?? []}
      traces={tracesQuery.data ?? []}
      hasReportData={hasReportData}
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
      contextUsage={effectiveUsage}
      quotedSnippet={quotedForChat}
      pendingCommentRefs={pendingCommentRefs}
      prefill={composerPrefill}
      onClearQuote={() => setQuotedForChat(null)}
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
        await compactSession(sessionId);
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
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

  const reportPane =
    docQuery.data && effectiveDocId ? (
      <SessionReportPane
        docId={effectiveDocId}
        document={docQuery.data.document}
        computed={docQuery.data.computed}
        currentRev={docQuery.data.current_rev}
        viewMode={prefs.reportViewMode}
        ephemeralPreviews={ephemeralPreviews}
        comments={commentsQuery.data ?? []}
        activeActorsBySection={activeActorsBySection}
        openCommentsCount={openCommentsCount}
        isMobile={isMobile}
        rightSidebarOpen={prefs.rightSidebarOpen}
        onToggleRightSidebar={toggleRightSidebar}
        onViewModeChange={(mode) => updatePrefs({ reportViewMode: mode })}
        onJumpToSection={jumpToSection}
        onShowComments={showComments}
        onOpenCommentForBlock={(anchor) => {
          setFloatingComment(anchor);
        }}
        onOpenCommentThread={openCommentThread}
        onAskAgentWithQuote={handleAskAgentWithQuote}
      />
    ) : null;

  const rightPane = (
    <SessionRightPane
      title={sessionQuery.data?.title ?? "加载中..."}
      runStatus={effectiveRunStatus}
      openCommentsCount={openCommentsCount}
      activeTab={rightTab}
      onTabChange={setRightTab}
      chatPane={chatPane}
      commentsPane={commentsPane}
    />
  );

  return (
    <>
      <SessionWorkspaceLayout
        isMobile={isMobile}
        hasReportData={hasReportData}
        sessionTitle={sessionQuery.data?.title ?? "加载中..."}
        runStatus={effectiveRunStatus}
        leftSidebarOpen={leftSidebarOpen}
        toggleSidebar={toggleSidebar}
        rightSidebarOpen={prefs.rightSidebarOpen}
        rightPanelPct={prefs.rightPanelPct}
        rightPanelAnimating={rightPanelAnimating}
        rightPanelRef={rightPanelRef}
        onChatPanelResize={handleChatPanelResize}
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
          existingCount={
            floatingComment
              ? (commentsQuery.data ?? []).filter(
                  (thread) =>
                    thread.block_id === floatingComment.blockId &&
                    thread.status !== "archived",
                ).length
              : 0
          }
          onClose={() => setFloatingComment(null)}
          onAddComment={async (blockId, body, quote) => {
            const thread = await createSessionComment(sessionId, {
              block_id: blockId,
              body,
              quote,
            });
            void queryClient.invalidateQueries({
              queryKey: ["comments", effectiveDocId],
            });
            setActiveCommentThreadId(thread.thread_id);
            showComments();
          }}
        />
      )}
    </>
  );
}

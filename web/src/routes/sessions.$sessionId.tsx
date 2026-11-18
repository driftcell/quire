import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import {
  BookOpenIcon,
  MessageSquareQuoteIcon,
  PanelLeftIcon,
  PanelRightIcon,
  PrinterIcon,
  ScrollTextIcon,
} from "lucide-react";
import {
  createComment,
  getDocument,
  listComments,
  replyComment,
  updateCommentStatus,
} from "@/api/quire";
import {
  answerQuestion,
  compactSession,
  getSession,
  interruptSession,
  postMessage,
  retrySession,
  uploadAttachment,
} from "@/api/runtime";
import { subscribeDocumentEvents, subscribeSessionEvents } from "@/api/sse";
import { PromptComposer } from "@/components/chat/PromptComposer";
import {
  SessionStreamView,
  type LiveAgentTrace,
  type LiveSubagentRun,
} from "@/components/chat/SessionStreamView";
import { CommentsDrawer } from "@/components/report/CommentsDrawer";
import { ReportViewer } from "@/components/report/ReportViewer";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { useSidebar } from "@/components/ui/sidebar";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type {
  CommentStatus,
  EphemeralPreviewEvent,
} from "@/contracts/report-protocol";
import type {
  ActiveRunStatus,
  ContextUsage,
  PendingQuestion,
  QuestionAnswer,
  SessionEvent,
} from "@/contracts/session-protocol";
import { useIsMobile } from "@/hooks/use-mobile";
import { loadPreferences, savePreferences } from "@/lib/preferences";

export const Route = createFileRoute("/sessions/$sessionId")({
  component: SessionWorkspaceRoute,
});

function emptyLiveTrace(agentId: string): LiveAgentTrace {
  return {
    agentId,
    thinking: "",
    text: "",
    tools: [],
    isStreaming: true,
  };
}

function SessionWorkspaceRoute() {
  const { sessionId } = Route.useParams();
  return <SessionWorkspace key={sessionId} sessionId={sessionId} />;
}

function SessionWorkspace({ sessionId }: { sessionId: string }) {
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const { toggleSidebar } = useSidebar();

  const [prefs, setPrefs] = useState(() => loadPreferences());
  const updatePrefs = useCallback((patch: Partial<typeof prefs>) => {
    setPrefs((prev) => {
      const next = { ...prev, ...patch };
      savePreferences(next);
      return next;
    });
  }, []);

  const sessionQuery = useQuery({
    queryKey: ["session", sessionId],
    queryFn: () => getSession(sessionId),
  });

  const [liveDocId, setLiveDocId] = useState<string | null>(null);
  const [liveRunStatus, setLiveRunStatus] = useState<ActiveRunStatus | null>(
    null,
  );
  const [liveUsage, setLiveUsage] = useState<ContextUsage | null>(null);
  const [liveMain, setLiveMain] = useState<LiveAgentTrace | null>(null);
  const [liveSubagents, setLiveSubagents] = useState<LiveSubagentRun[]>([]);
  const [livePendingQuestion, setLivePendingQuestion] =
    useState<PendingQuestion | null>(null);
  const [resolvedQuestion, setResolvedQuestion] = useState<{
    question: PendingQuestion;
    status: "answered" | "skipped" | "dismissed";
    answers?: QuestionAnswer[];
  } | null>(null);
  const [isAnsweringQuestion, setIsAnsweringQuestion] = useState(false);

  const [ephemeralPreviews, setEphemeralPreviews] = useState<
    Map<string, EphemeralPreviewEvent>
  >(() => new Map());
  const previewTimersRef = useRef<Map<string, number>>(new Map());

  const [commentsOpen, setCommentsOpen] = useState(false);
  const [selectedCommentBlock, setSelectedCommentBlock] = useState<
    string | null
  >(null);
  const [mobileTab, setMobileTab] = useState<"report" | "chat">("report");

  const effectiveDocId =
    liveDocId ??
    sessionQuery.data?.doc_id ??
    sessionQuery.data?.linked_doc_id ??
    null;

  const effectiveRunStatus: ActiveRunStatus =
    liveRunStatus ?? sessionQuery.data?.active_run_status ?? "idle";

  const effectiveUsage: ContextUsage | undefined =
    liveUsage ?? sessionQuery.data?.context_usage;

  const effectivePendingQuestion: PendingQuestion | null =
    livePendingQuestion ?? sessionQuery.data?.pending_question ?? null;

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

  // Subscribe to Session SSE
  const afterSeq = sessionQuery.data?.last_event_seq ?? 0;
  useEffect(() => {
    if (!sessionQuery.isSuccess) return undefined;

    const handleEvent = (ev: SessionEvent) => {
      if (ev.event === "doc_linked") {
        setLiveDocId(ev.doc_id);
        void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      } else if (ev.event === "turn_started") {
        setLiveRunStatus("running");
        setLiveMain(emptyLiveTrace("main"));
        setLiveSubagents([]);
      } else if (ev.event === "thinking_delta") {
        if (ev.parent_run_id) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              sub.agentName === ev.agent_id ||
              sub.subagentRunId === ev.parent_run_id
                ? {
                    ...sub,
                    trace: {
                      ...sub.trace,
                      thinking: sub.trace.thinking + ev.delta,
                    },
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) => ({
            ...(prev ?? emptyLiveTrace("main")),
            thinking: (prev?.thinking ?? "") + ev.delta,
          }));
        }
      } else if (ev.event === "text_delta") {
        if (ev.parent_run_id) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              sub.agentName === ev.agent_id
                ? {
                    ...sub,
                    trace: { ...sub.trace, text: sub.trace.text + ev.delta },
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) => ({
            ...(prev ?? emptyLiveTrace("main")),
            text: (prev?.text ?? "") + ev.delta,
          }));
        }
      } else if (ev.event === "task_focus") {
        const focus = {
          title: ev.title,
          detail: ev.detail ?? undefined,
          sectionId: ev.section_id ?? undefined,
        };
        if (ev.parent_run_id) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              sub.agentName === ev.agent_id
                ? { ...sub, trace: { ...sub.trace, taskFocus: focus } }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) => ({
            ...(prev ?? emptyLiveTrace("main")),
            taskFocus: focus,
          }));
        }
      } else if (ev.event === "tool_call_started") {
        const toolItem = {
          toolCallId: ev.tool_call_id,
          toolName: ev.tool_name,
          status: "running" as const,
          inputPreview: ev.input_preview,
          truncated: ev.truncated,
        };
        setLiveMain((prev) => ({
          ...(prev ?? emptyLiveTrace("main")),
          tools: [...(prev?.tools ?? []), toolItem],
        }));
      } else if (ev.event === "tool_call_finished") {
        setLiveMain((prev) =>
          prev
            ? {
                ...prev,
                tools: prev.tools.map((t) =>
                  t.toolCallId === ev.tool_call_id
                    ? {
                        ...t,
                        status:
                          ev.status === "failed" || ev.status === "error"
                            ? "failed"
                            : "completed",
                        outputPreview: ev.output_preview,
                        durationMs: ev.duration_ms,
                      }
                    : t,
                ),
              }
            : prev,
        );
      } else if (ev.event === "subagent_started") {
        setLiveSubagents((prev) => [
          ...prev,
          {
            subagentRunId: ev.subagent_run_id,
            agentName: ev.agent_name,
            task: ev.task,
            sectionId: ev.section_id ?? undefined,
            status: "running",
            trace: emptyLiveTrace(ev.agent_name),
          },
        ]);
      } else if (ev.event === "subagent_finished") {
        setLiveSubagents((prev) =>
          prev.map((sub) =>
            sub.subagentRunId === ev.subagent_run_id
              ? {
                  ...sub,
                  status: ev.status === "failed" ? "failed" : "completed",
                  summary: ev.summary,
                  trace: { ...sub.trace, isStreaming: false },
                }
              : sub,
          ),
        );
      } else if (ev.event === "context_usage") {
        setLiveUsage({
          input_tokens: ev.input_tokens,
          output_tokens: ev.output_tokens,
          cached_tokens: ev.cached_tokens,
          context_used: ev.context_used,
          context_limit: ev.context_limit,
        });
      } else if (ev.event === "questionnaire_requested") {
        setLiveRunStatus("awaiting_input");
        setLivePendingQuestion({
          question_id: ev.question_id,
          run_id: ev.run_id,
          questions: ev.questions,
          created_at: ev.timestamp,
        });
        setResolvedQuestion(null);
      } else if (ev.event === "questionnaire_answered") {
        setLivePendingQuestion((curr) => {
          if (curr) {
            setResolvedQuestion({
              question: curr,
              status: ev.skipped_all ? "skipped" : "answered",
              answers: ev.answers,
            });
          }
          return null;
        });
      } else if (ev.event === "questionnaire_dismissed") {
        setLivePendingQuestion((curr) => {
          if (curr) {
            setResolvedQuestion({ question: curr, status: "dismissed" });
          }
          return null;
        });
      } else if (ev.event === "turn_finished") {
        setLiveRunStatus("idle");
        setLiveMain(null);
        setLiveSubagents([]);
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
        void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      } else if (ev.event === "turn_failed") {
        setLiveRunStatus(ev.recoverable ? "interrupted" : "failed");
        setLiveMain((prev) => (prev ? { ...prev, isStreaming: false } : null));
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
      } else if (ev.event === "sync_required") {
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
      }
    };

    return subscribeSessionEvents(sessionId, afterSeq, {
      onEvent: handleEvent,
    });
  }, [sessionId, sessionQuery.isSuccess, afterSeq, queryClient]);

  // Subscribe to Report SSE once doc_id is bound
  const currentRev = docQuery.data?.current_rev ?? 0;
  useEffect(() => {
    if (!effectiveDocId || !docQuery.isSuccess) return undefined;
    const timers = previewTimersRef.current;

    const clearPreviewIds = (ids: string[]) => {
      if (ids.length === 0) return;
      setEphemeralPreviews((prev) => {
        const next = new Map(prev);
        for (const id of ids) {
          next.delete(id);
          const timer = timers.get(id);
          if (timer !== undefined) {
            window.clearTimeout(timer);
            timers.delete(id);
          }
        }
        return next;
      });
    };

    const unsub = subscribeDocumentEvents(effectiveDocId, currentRev, {
      onRevision(delta) {
        if (delta.cleared_preview_ids && delta.cleared_preview_ids.length > 0) {
          clearPreviewIds(delta.cleared_preview_ids);
        }
        void queryClient.invalidateQueries({
          queryKey: ["doc", effectiveDocId],
        });
      },
      onEphemeralPreview(preview) {
        setEphemeralPreviews((prev) => {
          const existing = prev.get(preview.preview_id);
          if (
            existing?.seq !== undefined &&
            existing.seq !== null &&
            preview.seq !== undefined &&
            preview.seq !== null &&
            preview.seq < existing.seq
          ) {
            return prev;
          }
          const next = new Map(prev);
          if (preview.status === "streaming") {
            next.set(preview.preview_id, preview);
          } else {
            next.delete(preview.preview_id);
          }
          return next;
        });

        const oldTimer = timers.get(preview.preview_id);
        if (oldTimer !== undefined) {
          window.clearTimeout(oldTimer);
        }
        const ttl = preview.ttl_ms ?? 15_000;
        const timerId = window.setTimeout(() => {
          clearPreviewIds([preview.preview_id]);
        }, ttl);
        timers.set(preview.preview_id, timerId);
      },
      onEphemeralClear(clear) {
        clearPreviewIds(clear.preview_ids);
      },
      onAgentPresence() {},
      onCommentUpdated() {
        void queryClient.invalidateQueries({
          queryKey: ["comments", effectiveDocId],
        });
      },
      onSyncRequired() {
        void queryClient.invalidateQueries({
          queryKey: ["doc", effectiveDocId],
        });
      },
    });

    return () => {
      unsub();
      for (const timer of timers.values()) {
        window.clearTimeout(timer);
      }
      timers.clear();
    };
  }, [effectiveDocId, docQuery.isSuccess, currentRev, queryClient]);

  const blockCount = docQuery.data?.document.blocks.length ?? 0;
  const hasReportData = useMemo(() => {
    if (!effectiveDocId) return false;
    return blockCount > 0 || ephemeralPreviews.size > 0;
  }, [effectiveDocId, blockCount, ephemeralPreviews.size]);

  const handleSend = async (text: string, files: File[]) => {
    const attachmentIds: string[] = [];
    for (const file of files) {
      const uploaded = await uploadAttachment(sessionId, file);
      attachmentIds.push(uploaded.attachment_id);
    }
    await postMessage(sessionId, {
      client_msg_id: `msg_${crypto.randomUUID()}`,
      content: text || `请处理已上传的 ${files.length} 个附件。`,
      attachment_ids: attachmentIds,
    });
    setLivePendingQuestion(null);
    void queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
  };

  const handleAnswerQuestion = async (
    questionId: string,
    answers: QuestionAnswer[],
    skipped: boolean,
  ) => {
    setIsAnsweringQuestion(true);
    try {
      await answerQuestion(sessionId, questionId, { answers, skipped });
      setLivePendingQuestion(null);
      void queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
    } finally {
      setIsAnsweringQuestion(false);
    }
  };

  const jumpToSection = (sectionId: string) => {
    const el = window.document.getElementById(`block-${sectionId}`);
    el?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const openCommentsCount =
    commentsQuery.data?.filter((c) => c.status !== "archived").length ?? 0;

  const chatPane = (
    <div className="flex h-full flex-col bg-background">
      <div className="flex-1 overflow-y-auto px-4">
        <div
          className={hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"}
        >
          <SessionStreamView
            sessionId={sessionId}
            messages={sessionQuery.data?.messages ?? []}
            liveMain={liveMain}
            liveSubagents={liveSubagents}
            pendingQuestion={effectivePendingQuestion}
            resolvedQuestion={resolvedQuestion}
            isAnsweringQuestion={isAnsweringQuestion}
            onAnswerQuestion={(qId, answers, skipped) => {
              void handleAnswerQuestion(qId, answers, skipped);
            }}
            onJumpToSection={jumpToSection}
          />
        </div>
      </div>
      <div className="shrink-0 border-t bg-background/95 p-3">
        <div
          className={hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"}
        >
          <PromptComposer
            runStatus={effectiveRunStatus}
            contextUsage={effectiveUsage}
            onSend={handleSend}
            onInterrupt={async () => {
              await interruptSession(sessionId);
              void queryClient.invalidateQueries({
                queryKey: ["session", sessionId],
              });
            }}
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
        </div>
      </div>
    </div>
  );

  const reportPane =
    docQuery.data && effectiveDocId ? (
      <div className="h-full overflow-y-auto bg-muted/20">
        <ReportViewer
          document={docQuery.data.document}
          computed={docQuery.data.computed}
          viewMode={prefs.reportViewMode}
          ephemeralPreviews={ephemeralPreviews}
          comments={commentsQuery.data}
          onOpenCommentForBlock={(blockId) => {
            setSelectedCommentBlock(blockId);
            setCommentsOpen(true);
          }}
        />
      </div>
    ) : null;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="切换左侧会话栏"
            onClick={toggleSidebar}
          >
            <PanelLeftIcon className="size-4" />
          </Button>
          <span className="truncate text-sm font-medium">
            {sessionQuery.data?.title ?? "加载中..."}
          </span>
          {effectiveRunStatus === "running" && (
            <Badge variant="secondary" className="text-[11px]">
              生成中
            </Badge>
          )}
        </div>

        <div className="flex items-center gap-1">
          {hasReportData && effectiveDocId && (
            <>
              <div className="hidden items-center rounded-lg border bg-muted/40 p-0.5 sm:flex">
                <Button
                  type="button"
                  size="xs"
                  variant={
                    prefs.reportViewMode === "continuous" ? "default" : "ghost"
                  }
                  onClick={() => updatePrefs({ reportViewMode: "continuous" })}
                >
                  <ScrollTextIcon className="mr-1 size-3.5" />
                  连续
                </Button>
                <Button
                  type="button"
                  size="xs"
                  variant={
                    prefs.reportViewMode === "paged" ? "default" : "ghost"
                  }
                  onClick={() => updatePrefs({ reportViewMode: "paged" })}
                >
                  <BookOpenIcon className="mr-1 size-3.5" />
                  分页
                </Button>
              </div>

              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                aria-label="批注列表"
                className="relative"
                onClick={() => {
                  setSelectedCommentBlock(null);
                  setCommentsOpen(true);
                }}
              >
                <MessageSquareQuoteIcon className="size-4" />
                {openCommentsCount > 0 && (
                  <span className="absolute -top-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full bg-amber-500 text-[10px] font-bold text-white">
                    {openCommentsCount}
                  </span>
                )}
              </Button>

              <Button
                size="icon-sm"
                variant="ghost"
                aria-label="导出 PDF / 打印视图"
                nativeButton={false}
                render={
                  <Link
                    to="/docs/$docId/print"
                    params={{ docId: effectiveDocId }}
                    target="_blank"
                  />
                }
              >
                <PrinterIcon className="size-4" />
              </Button>

              {!isMobile && (
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  aria-label="切换右侧对话栏"
                  onClick={() =>
                    updatePrefs({ rightSidebarOpen: !prefs.rightSidebarOpen })
                  }
                >
                  <PanelRightIcon className="size-4" />
                </Button>
              )}
            </>
          )}
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden">
        {!hasReportData ? (
          chatPane
        ) : isMobile ? (
          <Tabs
            value={mobileTab}
            onValueChange={(v) => {
              if (v === "report" || v === "chat") {
                setMobileTab(v);
              }
            }}
            className="flex h-full flex-col"
          >
            <div className="border-b px-3 py-1.5">
              <TabsList className="grid w-full grid-cols-2">
                <TabsTrigger value="report">报告预览</TabsTrigger>
                <TabsTrigger value="chat">Agent 对话</TabsTrigger>
              </TabsList>
            </div>
            <TabsContent
              value="report"
              className="min-h-0 flex-1 overflow-hidden"
            >
              {reportPane}
            </TabsContent>
            <TabsContent
              value="chat"
              className="min-h-0 flex-1 overflow-hidden"
            >
              {chatPane}
            </TabsContent>
          </Tabs>
        ) : prefs.rightSidebarOpen ? (
          <ResizablePanelGroup orientation="horizontal" className="h-full">
            <ResizablePanel
              defaultSize={100 - prefs.rightPanelPct}
              minSize={35}
            >
              {reportPane}
            </ResizablePanel>
            <ResizableHandle withHandle />
            <ResizablePanel
              defaultSize={prefs.rightPanelPct}
              minSize={22}
              maxSize={55}
              onResize={(size) => {
                const pct = typeof size === "number" ? size : size.asPercentage;
                if (pct >= 20 && pct <= 60) {
                  updatePrefs({ rightPanelPct: Math.round(pct) });
                }
              }}
            >
              {chatPane}
            </ResizablePanel>
          </ResizablePanelGroup>
        ) : (
          reportPane
        )}
      </div>

      {effectiveDocId && (
        <CommentsDrawer
          open={commentsOpen}
          onOpenChange={setCommentsOpen}
          threads={commentsQuery.data ?? []}
          selectedBlockId={selectedCommentBlock}
          onAddComment={async (blockId, body) => {
            await createComment(effectiveDocId, {
              block_id: blockId,
              author: "reviewer",
              body,
            });
            void queryClient.invalidateQueries({
              queryKey: ["comments", effectiveDocId],
            });
          }}
          onReply={async (threadId, body) => {
            await replyComment(effectiveDocId, threadId, {
              author: "reviewer",
              body,
            });
            void queryClient.invalidateQueries({
              queryKey: ["comments", effectiveDocId],
            });
          }}
          onUpdateStatus={async (threadId, status: CommentStatus) => {
            await updateCommentStatus(effectiveDocId, threadId, status);
            void queryClient.invalidateQueries({
              queryKey: ["comments", effectiveDocId],
            });
          }}
          onSelectBlock={(blockId) => {
            jumpToSection(blockId);
          }}
        />
      )}
    </div>
  );
}

import {
  startTransition,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import {
  BookOpenIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
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
  listSessionTraces,
  postMessage,
  retrySession,
  uploadAttachment,
} from "@/api/runtime";
import { subscribeDocumentEvents, subscribeSessionEvents } from "@/api/sse";
import {
  PromptComposer,
  type QuotedChatSnippet,
} from "@/components/chat/PromptComposer";
import {
  SessionStreamView,
  type LiveAgentTrace,
  type LiveSubagentRun,
} from "@/components/chat/SessionStreamView";
import { CommentsDrawer } from "@/components/report/CommentsDrawer";
import {
  FloatingCommentPopover,
  type FloatingCommentAnchor,
} from "@/components/report/FloatingCommentPopover";
import { PageChromeDialog } from "@/components/report/PageChromeDialog";
import { ReportViewer } from "@/components/report/ReportViewer";
import { TocOutline } from "@/components/report/TocOutline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { useSidebar } from "@/components/ui/sidebar";
import { Skeleton } from "@/components/ui/skeleton";
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
  SessionMessage,
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

function matchesSubagent(
  sub: LiveSubagentRun,
  agentId: string,
  parentRunId?: string | null,
): boolean {
  return (
    (parentRunId !== undefined &&
      parentRunId !== null &&
      sub.subagentRunId === parentRunId) ||
    sub.agentName === agentId
  );
}

function SessionWorkspaceRoute() {
  const { sessionId } = Route.useParams();
  return <SessionWorkspace key={sessionId} sessionId={sessionId} />;
}

function SessionWorkspace({ sessionId }: { sessionId: string }) {
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const { open: leftSidebarOpen, toggleSidebar } = useSidebar();

  const [prefs, setPrefs] = useState(() => loadPreferences());
  const updatePrefs = useCallback((patch: Partial<typeof prefs>) => {
    startTransition(() => {
      setPrefs((prev) => {
        const next = { ...prev, ...patch };
        savePreferences(next);
        return next;
      });
    });
  }, []);

  const sessionQuery = useQuery({
    queryKey: ["session", sessionId],
    queryFn: () => getSession(sessionId),
  });

  const tracesQuery = useQuery({
    queryKey: ["session-traces", sessionId],
    queryFn: () => listSessionTraces(sessionId),
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
  const pendingClearedByRevRef = useRef<Map<number, string[]>>(new Map());

  const [activeActorsBySection, setActiveActorsBySection] = useState<
    Map<string, string[]>
  >(() => new Map());
  const presenceTimersRef = useRef<Map<string, number>>(new Map());

  const [commentsOpen, setCommentsOpen] = useState(false);
  const [selectedCommentBlock, setSelectedCommentBlock] = useState<
    string | null
  >(null);
  const [floatingComment, setFloatingComment] =
    useState<FloatingCommentAnchor | null>(null);
  const [mobileTab, setMobileTab] = useState<"report" | "chat">("report");
  const [optimisticMessages, setOptimisticMessages] = useState<
    SessionMessage[]
  >([]);
  const [quotedForChat, setQuotedForChat] = useState<QuotedChatSnippet | null>(
    null,
  );

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

  const afterSeqRef = useRef<number>(0);
  const lastEventSeq = sessionQuery.data?.last_event_seq ?? 0;
  useEffect(() => {
    afterSeqRef.current = lastEventSeq;
  }, [lastEventSeq]);

  useEffect(() => {
    if (!sessionQuery.isSuccess) return undefined;

    const handleEvent = (ev: SessionEvent) => {
      const isSubagentEvent =
        Boolean(ev.parent_run_id) ||
        (ev.agent_id !== "main" && ev.agent_id.length > 0);

      if (ev.event === "doc_linked") {
        setLiveDocId(ev.doc_id);
        void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      } else if (ev.event === "turn_started") {
        setLiveRunStatus("running");
        setLiveMain(emptyLiveTrace("main"));
        setLiveSubagents([]);
      } else if (ev.event === "thinking_delta") {
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.agent_id, ev.parent_run_id)
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
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.agent_id, ev.parent_run_id)
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
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.agent_id, ev.parent_run_id)
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
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.agent_id, ev.parent_run_id)
                ? {
                    ...sub,
                    trace: {
                      ...sub.trace,
                      tools: [...sub.trace.tools, toolItem],
                    },
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) => ({
            ...(prev ?? emptyLiveTrace("main")),
            tools: [...(prev?.tools ?? []), toolItem],
          }));
        }
      } else if (ev.event === "tool_call_finished") {
        const nextStatus: "completed" | "failed" =
          ev.status === "failed" || ev.status === "error"
            ? "failed"
            : "completed";
        const updateTools = (tools: LiveAgentTrace["tools"]) =>
          tools.map((t) =>
            t.toolCallId === ev.tool_call_id
              ? {
                  ...t,
                  status: nextStatus,
                  outputPreview: ev.output_preview,
                  durationMs: ev.duration_ms,
                }
              : t,
          );
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.agent_id, ev.parent_run_id)
                ? {
                    ...sub,
                    trace: {
                      ...sub.trace,
                      tools: updateTools(sub.trace.tools),
                    },
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) =>
            prev ? { ...prev, tools: updateTools(prev.tools) } : prev,
          );
        }
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
        void (async () => {
          await Promise.all([
            queryClient.refetchQueries({
              queryKey: ["session", sessionId],
            }),
            queryClient.refetchQueries({
              queryKey: ["session-traces", sessionId],
            }),
          ]);
          setLiveMain(null);
          setLiveSubagents([]);
        })();
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

    return subscribeSessionEvents(sessionId, afterSeqRef.current, {
      onEvent: handleEvent,
    });
  }, [sessionId, sessionQuery.isSuccess, queryClient]);

  const currentRevRef = useRef<number>(0);
  const currentRev = docQuery.data?.current_rev ?? 0;
  useEffect(() => {
    currentRevRef.current = currentRev;
    if (pendingClearedByRevRef.current.size === 0) return;
    const toClear: string[] = [];
    for (const [rev, ids] of pendingClearedByRevRef.current.entries()) {
      if (rev <= currentRev) {
        toClear.push(...ids);
        pendingClearedByRevRef.current.delete(rev);
      }
    }
    if (toClear.length > 0) {
      setEphemeralPreviews((prev) => {
        const next = new Map(prev);
        for (const id of toClear) {
          next.delete(id);
          const timer = previewTimersRef.current.get(id);
          if (timer !== undefined) {
            window.clearTimeout(timer);
            previewTimersRef.current.delete(id);
          }
        }
        return next;
      });
    }
  }, [currentRev]);

  useEffect(() => {
    if (!effectiveDocId || !docQuery.isSuccess) return undefined;
    const timers = previewTimersRef.current;
    const presenceTimers = presenceTimersRef.current;

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

    const removeActor = (actor: string) => {
      setActiveActorsBySection((prev) => {
        let changed = false;
        const next = new Map<string, string[]>();
        for (const [sec, actors] of prev.entries()) {
          const filtered = actors.filter((a) => a !== actor);
          if (filtered.length !== actors.length) {
            changed = true;
          }
          if (filtered.length > 0) {
            next.set(sec, filtered);
          }
        }
        return changed ? next : prev;
      });
    };

    const unsub = subscribeDocumentEvents(
      effectiveDocId,
      currentRevRef.current,
      {
        onRevision(delta) {
          if (
            delta.cleared_preview_ids &&
            delta.cleared_preview_ids.length > 0
          ) {
            if (currentRevRef.current >= delta.revision) {
              clearPreviewIds(delta.cleared_preview_ids);
            } else {
              const prevList =
                pendingClearedByRevRef.current.get(delta.revision) ?? [];
              pendingClearedByRevRef.current.set(delta.revision, [
                ...prevList,
                ...delta.cleared_preview_ids,
              ]);
            }
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
        onAgentPresence(presence) {
          const prevTimer = presenceTimers.get(presence.actor);
          if (prevTimer !== undefined) {
            window.clearTimeout(prevTimer);
            presenceTimers.delete(presence.actor);
          }
          if (presence.status === "idle" || !presence.section_id) {
            removeActor(presence.actor);
            return;
          }
          const sectionId = presence.section_id;
          setActiveActorsBySection((prev) => {
            const next = new Map<string, string[]>();
            for (const [sec, actors] of prev.entries()) {
              const filtered = actors.filter((a) => a !== presence.actor);
              if (filtered.length > 0) {
                next.set(sec, filtered);
              }
            }
            const curr = next.get(sectionId) ?? [];
            next.set(sectionId, [...curr, presence.actor]);
            return next;
          });
          const timerId = window.setTimeout(() => {
            removeActor(presence.actor);
            presenceTimers.delete(presence.actor);
          }, 10_000);
          presenceTimers.set(presence.actor, timerId);
        },
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
      },
    );

    return () => {
      unsub();
      for (const timer of timers.values()) {
        window.clearTimeout(timer);
      }
      timers.clear();
      for (const timer of presenceTimers.values()) {
        window.clearTimeout(timer);
      }
      presenceTimers.clear();
    };
  }, [effectiveDocId, docQuery.isSuccess, queryClient]);

  const blockCount = docQuery.data?.document.blocks.length ?? 0;
  const hasReportData = useMemo(() => {
    if (!effectiveDocId) return false;
    return blockCount > 0 || ephemeralPreviews.size > 0;
  }, [effectiveDocId, blockCount, ephemeralPreviews.size]);

  const handleSend = async (text: string, files: File[]) => {
    const content = text || `请处理已上传的 ${files.length} 个附件。`;
    const tempId = `opt_${crypto.randomUUID()}`;
    const optimistic: SessionMessage = {
      message_id: tempId,
      session_id: sessionId,
      role: "user",
      content,
      created_at: new Date().toISOString(),
    };
    setOptimisticMessages((prev) => [...prev, optimistic]);
    try {
      const attachmentIds: string[] = [];
      for (const file of files) {
        const uploaded = await uploadAttachment(sessionId, file);
        attachmentIds.push(uploaded.attachment_id);
      }
      await postMessage(sessionId, {
        client_msg_id: `msg_${crypto.randomUUID()}`,
        content,
        attachment_ids: attachmentIds,
      });
      setLivePendingQuestion(null);
      void queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
    } catch (err) {
      setOptimisticMessages((prev) =>
        prev.filter((m) => m.message_id !== tempId),
      );
      throw err;
    }
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

  const jumpToSection = useCallback((sectionId: string) => {
    const el = window.document.getElementById(`block-${sectionId}`);
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "center" });
    el.classList.remove("quire-flash-highlight");
    void el.offsetWidth;
    el.classList.add("quire-flash-highlight");
    window.setTimeout(() => {
      el.classList.remove("quire-flash-highlight");
    }, 1600);
  }, []);

  const openCommentsCount =
    commentsQuery.data?.filter((c) => c.status !== "archived").length ?? 0;

  const chatPane = (
    <div
      className={
        hasReportData && !isMobile
          ? "flex h-full min-w-0 flex-col overflow-hidden rounded-xl border bg-background shadow-2xs"
          : "flex h-full min-w-0 flex-col bg-background"
      }
    >
      {hasReportData && !isMobile && (
        <div className="flex h-11 shrink-0 items-center justify-between gap-2 border-b px-3">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate text-sm font-medium">
              {sessionQuery.data?.title ?? "加载中..."}
            </span>
            {effectiveRunStatus === "running" && (
              <Badge variant="secondary" className="text-[11px]">
                生成中
              </Badge>
            )}
          </div>
        </div>
      )}

      <div className="min-h-0 flex-1">
        {sessionQuery.isLoading ? (
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
            messages={sessionQuery.data?.messages ?? []}
            optimisticMessages={optimisticMessages}
            contentClassName={
              hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"
            }
            onInterrupt={async () => {
              await interruptSession(sessionId);
              void queryClient.invalidateQueries({
                queryKey: ["session", sessionId],
              });
            }}
            attachments={sessionQuery.data?.attachments ?? []}
            traces={tracesQuery.data ?? []}
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
        )}
      </div>

      <div className="shrink-0 border-t bg-background/95 p-3">
        <div
          className={hasReportData ? "mx-auto max-w-full" : "mx-auto max-w-3xl"}
        >
          <PromptComposer
            runStatus={effectiveRunStatus}
            contextUsage={effectiveUsage}
            quotedSnippet={quotedForChat}
            onClearQuote={() => setQuotedForChat(null)}
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
      <div
        className={
          isMobile
            ? "relative flex h-full min-w-0 flex-col overflow-hidden bg-background"
            : "relative flex h-full min-w-0 flex-col overflow-hidden rounded-xl border bg-background shadow-2xs"
        }
      >
        {!isMobile && (
          <div className="flex h-11 shrink-0 items-center justify-between gap-2 border-b bg-background/95 px-3">
            <div className="flex min-w-0 items-center gap-2">
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                aria-label={
                  leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"
                }
                onClick={toggleSidebar}
              >
                <PanelLeftIcon className="size-4 text-muted-foreground" />
              </Button>
              <span className="truncate text-sm font-medium">
                {docQuery.data.document.cover.title || effectiveDocId}
              </span>
              <Badge
                variant="outline"
                className="shrink-0 text-[11px] tabular-nums"
              >
                版本 {docQuery.data.current_rev}
              </Badge>
            </div>

            <div className="flex items-center gap-1">
              <div className="flex items-center rounded-lg border bg-muted/40 p-0.5">
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

              <TocOutline
                placement="bottom"
                nodes={docQuery.data.computed.toc_tree}
                activeActorsBySection={activeActorsBySection}
                comments={commentsQuery.data ?? []}
                onJumpToSection={jumpToSection}
              />

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
                  <span className="absolute -top-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full bg-amber-500 text-[10px] font-bold text-white tabular-nums">
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

              {prefs.reportViewMode === "paged" && effectiveDocId && (
                <PageChromeDialog
                  docId={effectiveDocId}
                  themePageChrome={docQuery.data?.document.theme.page_chrome}
                />
              )}

              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                aria-label={
                  prefs.rightSidebarOpen ? "收起右侧对话栏" : "展开右侧对话栏"
                }
                onClick={() =>
                  updatePrefs({ rightSidebarOpen: !prefs.rightSidebarOpen })
                }
              >
                <PanelRightIcon className="size-4 text-muted-foreground" />
              </Button>
            </div>
          </div>
        )}

        <div className="flex-1 overflow-y-auto overscroll-contain">
          <ReportViewer
            document={docQuery.data.document}
            computed={docQuery.data.computed}
            viewMode={prefs.reportViewMode}
            ephemeralPreviews={ephemeralPreviews}
            comments={commentsQuery.data}
            onOpenCommentForBlock={(anchor) => {
              setFloatingComment(anchor);
            }}
          />
        </div>
      </div>
    ) : null;

  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden">
      {(!hasReportData || isMobile) && (
        <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b bg-background px-3">
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
        </header>
      )}

      <div
        className={
          hasReportData && !isMobile
            ? "relative min-h-0 min-w-0 flex-1 overflow-hidden p-2"
            : "relative min-h-0 min-w-0 flex-1 overflow-hidden"
        }
      >
        {!isMobile && (
          <button
            type="button"
            title={leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"}
            aria-label={leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"}
            onClick={toggleSidebar}
            className="focus-visible:ring-ring absolute top-1/2 left-0 z-30 flex h-12 w-4 -translate-y-1/2 items-center justify-center rounded-r-md border border-l-0 bg-background/90 text-muted-foreground opacity-60 shadow-xs backdrop-blur transition-opacity hover:w-5 hover:text-foreground hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:outline-none"
          >
            {leftSidebarOpen ? (
              <ChevronLeftIcon className="size-3.5" />
            ) : (
              <ChevronRightIcon className="size-3.5" />
            )}
          </button>
        )}
        {hasReportData && !isMobile && (
          <button
            type="button"
            title={prefs.rightSidebarOpen ? "收起右侧对话栏" : "展开右侧对话栏"}
            aria-label={
              prefs.rightSidebarOpen ? "收起右侧对话栏" : "展开右侧对话栏"
            }
            onClick={() =>
              updatePrefs({ rightSidebarOpen: !prefs.rightSidebarOpen })
            }
            className="focus-visible:ring-ring absolute top-1/2 right-0 z-30 flex h-12 w-4 -translate-y-1/2 items-center justify-center rounded-l-md border border-r-0 bg-background/90 text-muted-foreground opacity-60 shadow-xs backdrop-blur transition-opacity hover:w-5 hover:text-foreground hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:outline-none"
          >
            {prefs.rightSidebarOpen ? (
              <ChevronRightIcon className="size-3.5" />
            ) : (
              <ChevronLeftIcon className="size-3.5" />
            )}
          </button>
        )}
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
            <div className="border-b bg-background px-3 py-1.5">
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
          <ResizablePanelGroup
            orientation="horizontal"
            className="h-full min-w-0 gap-1.5"
          >
            <ResizablePanel
              defaultSize={`${100 - prefs.rightPanelPct}%`}
              minSize="35%"
              className="min-w-0"
            >
              {reportPane}
            </ResizablePanel>
            <ResizableHandle
              withHandle
              className="w-1.5 bg-transparent after:hidden"
            />
            <ResizablePanel
              defaultSize={`${prefs.rightPanelPct}%`}
              minSize="22%"
              maxSize="55%"
              className="min-w-0"
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
        <>
          <FloatingCommentPopover
            anchor={floatingComment}
            threads={commentsQuery.data ?? []}
            onClose={() => setFloatingComment(null)}
            onAddComment={async (blockId, body, quote) => {
              await createComment(effectiveDocId, {
                block_id: blockId,
                author: "reviewer",
                body,
                quote,
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
            onOpenDrawer={() => {
              setSelectedCommentBlock(floatingComment?.blockId ?? null);
              setCommentsOpen(true);
            }}
          />

          <CommentsDrawer
            docId={effectiveDocId}
            open={commentsOpen}
            onOpenChange={setCommentsOpen}
            threads={commentsQuery.data ?? []}
            selectedBlockId={selectedCommentBlock}
            onAddComment={async (blockId, body, quote) => {
              await createComment(effectiveDocId, {
                block_id: blockId,
                author: "reviewer",
                body,
                quote,
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
        </>
      )}
    </div>
  );
}

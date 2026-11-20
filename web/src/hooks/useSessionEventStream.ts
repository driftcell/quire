import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { subscribeSessionEvents } from "@/api/sse";
import type {
  ActiveRunStatus,
  ContextUsage,
  PendingQuestion,
  QuestionAnswer,
  SessionDetail,
  SessionEvent,
} from "@/contracts/session-protocol";
import {
  appendSubagentSegment,
  appendTextSegment,
  appendThinkingSegment,
  appendToolSegment,
  emptyLiveTrace,
  finishToolSegment,
  type LiveAgentTrace,
  type LiveSubagentRun,
} from "@/lib/liveTrace";

function matchesSubagent(sub: LiveSubagentRun, runId: string): boolean {
  return sub.subagentRunId === runId;
}

export interface ResolvedQuestionEntry {
  question: PendingQuestion;
  status: "answered" | "skipped" | "dismissed";
  answers?: QuestionAnswer[];
}

export interface UseSessionEventStreamOptions {
  sessionId: string;
  enabled: boolean;
  lastEventSeq: number;
  serverPendingQuestion?: PendingQuestion | null;
  serverResolvedQuestions?: SessionDetail["resolved_questions"];
}

export function useSessionEventStream({
  sessionId,
  enabled,
  lastEventSeq,
  serverPendingQuestion,
  serverResolvedQuestions,
}: UseSessionEventStreamOptions) {
  const queryClient = useQueryClient();
  const [liveDocId, setLiveDocId] = useState<string | null>(null);
  const [liveRunStatus, setLiveRunStatus] = useState<ActiveRunStatus | null>(
    null,
  );
  const [liveUsage, setLiveUsage] = useState<ContextUsage | null>(null);
  const [liveMain, setLiveMain] = useState<LiveAgentTrace | null>(null);
  const [liveSubagents, setLiveSubagents] = useState<LiveSubagentRun[]>([]);
  const [livePendingQuestion, setLivePendingQuestion] =
    useState<PendingQuestion | null>(null);
  const [resolvedQuestions, setResolvedQuestions] = useState<
    ResolvedQuestionEntry[]
  >([]);
  const [sseGeneration, setSseGeneration] = useState(0);

  const afterSeqRef = useRef<number>(0);
  useEffect(() => {
    afterSeqRef.current = lastEventSeq;
  }, [lastEventSeq]);

  const effectivePendingQuestion =
    livePendingQuestion ?? serverPendingQuestion ?? null;
  const effectivePendingQuestionRef = useRef<PendingQuestion | null>(
    effectivePendingQuestion,
  );
  useEffect(() => {
    effectivePendingQuestionRef.current = effectivePendingQuestion;
  }, [effectivePendingQuestion]);

  const recordResolvedQuestion = useCallback(
    (
      question: PendingQuestion,
      status: "answered" | "skipped" | "dismissed",
      answers?: QuestionAnswer[],
    ) => {
      setResolvedQuestions((prev) => {
        const next = prev.filter(
          (item) => item.question.question_id !== question.question_id,
        );
        next.push({ question, status, answers });
        return next;
      });
    },
    [],
  );

  const clearLivePendingQuestion = useCallback(() => {
    setLivePendingQuestion(null);
  }, []);

  const mergedResolvedQuestions = useMemo(() => {
    const byId = new Map<string, ResolvedQuestionEntry>();
    for (const item of serverResolvedQuestions ?? []) {
      byId.set(item.question.question_id, {
        question: item.question,
        status:
          item.status === "skipped" || item.status === "dismissed"
            ? item.status
            : "answered",
        answers: item.answers,
      });
    }
    for (const item of resolvedQuestions) {
      byId.set(item.question.question_id, item);
    }
    return Array.from(byId.values());
  }, [serverResolvedQuestions, resolvedQuestions]);

  useEffect(() => {
    if (!enabled) return undefined;

    const handleEvent = (ev: SessionEvent) => {
      if (ev.event !== "sync_required" && ev.seq > 0) {
        afterSeqRef.current = Math.max(afterSeqRef.current, ev.seq);
      }
      const isSubagentEvent =
        Boolean(ev.parent_run_id) ||
        (ev.agent_id !== "main" && ev.agent_id.length > 0);

      if (ev.event === "doc_linked") {
        setLiveDocId(ev.doc_id);
        void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      } else if (ev.event === "session_title_updated") {
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
        void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      } else if (ev.event === "user_message_added") {
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
      } else if (ev.event === "turn_started") {
        setLiveRunStatus("running");
        setLiveMain(emptyLiveTrace("main"));
        setLiveSubagents([]);
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
      } else if (ev.event === "thinking_delta") {
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.run_id)
                ? {
                    ...sub,
                    trace: appendThinkingSegment(sub.trace, ev.delta),
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) =>
            appendThinkingSegment(prev ?? emptyLiveTrace("main"), ev.delta),
          );
        }
      } else if (ev.event === "text_delta") {
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.run_id)
                ? {
                    ...sub,
                    trace: appendTextSegment(sub.trace, ev.delta),
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) =>
            appendTextSegment(prev ?? emptyLiveTrace("main"), ev.delta),
          );
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
              matchesSubagent(sub, ev.run_id)
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
              matchesSubagent(sub, ev.run_id)
                ? {
                    ...sub,
                    trace: appendToolSegment(sub.trace, toolItem),
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) =>
            appendToolSegment(prev ?? emptyLiveTrace("main"), toolItem),
          );
        }
      } else if (ev.event === "tool_call_finished") {
        const nextStatus: "completed" | "failed" =
          ev.status === "failed" || ev.status === "error"
            ? "failed"
            : "completed";
        const patch = {
          status: nextStatus,
          outputPreview: ev.output_preview,
          durationMs: ev.duration_ms,
        };
        if (isSubagentEvent) {
          setLiveSubagents((prev) =>
            prev.map((sub) =>
              matchesSubagent(sub, ev.run_id)
                ? {
                    ...sub,
                    trace: finishToolSegment(sub.trace, ev.tool_call_id, patch),
                  }
                : sub,
            ),
          );
        } else {
          setLiveMain((prev) =>
            prev ? finishToolSegment(prev, ev.tool_call_id, patch) : prev,
          );
        }
      } else if (ev.event === "subagent_started") {
        setLiveMain((prev) =>
          prev ? appendSubagentSegment(prev, ev.subagent_run_id) : prev,
        );
        setLiveSubagents((prev) => [
          ...prev,
          {
            subagentRunId: ev.subagent_run_id,
            agentName: ev.agent_name,
            displayName: ev.display_name ?? undefined,
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
          tool_call_id: ev.tool_call_id,
          questions: ev.questions,
          created_at: ev.timestamp,
        });
      } else if (ev.event === "questionnaire_answered") {
        const target = effectivePendingQuestionRef.current;
        if (target && target.question_id === ev.question_id) {
          recordResolvedQuestion(
            target,
            ev.skipped_all ? "skipped" : "answered",
            ev.answers,
          );
        }
        setLivePendingQuestion((curr) =>
          curr?.question_id === ev.question_id ? null : curr,
        );
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
        void queryClient.invalidateQueries({
          queryKey: ["session-traces", sessionId],
        });
      } else if (ev.event === "questionnaire_dismissed") {
        const target = effectivePendingQuestionRef.current;
        if (target && target.question_id === ev.question_id) {
          recordResolvedQuestion(target, "dismissed");
        }
        setLivePendingQuestion((curr) =>
          curr?.question_id === ev.question_id ? null : curr,
        );
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
        void queryClient.invalidateQueries({
          queryKey: ["session-traces", sessionId],
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
        void (async () => {
          try {
            await queryClient.refetchQueries({
              queryKey: ["session", sessionId],
            });
            const detail = queryClient.getQueryData<SessionDetail>([
              "session",
              sessionId,
            ]);
            if (detail && typeof detail.last_event_seq === "number") {
              afterSeqRef.current = Math.max(
                afterSeqRef.current,
                detail.last_event_seq,
              );
            }
          } catch {
            // Reconnect anyway; the server replays from the current cursor.
          } finally {
            setSseGeneration((generation) => generation + 1);
          }
        })();
      } else if (ev.event === "pending_queue_updated") {
        queryClient.setQueryData<SessionDetail>(
          ["session", sessionId],
          (current) =>
            current ? { ...current, pending_queue: ev.items } : current,
        );
        void queryClient.invalidateQueries({
          queryKey: ["session", sessionId],
        });
      }
    };

    return subscribeSessionEvents(sessionId, () => afterSeqRef.current, {
      onEvent: handleEvent,
    });
  }, [
    sessionId,
    enabled,
    queryClient,
    recordResolvedQuestion,
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- bumping generation intentionally tears down and recreates the SSE stream
    sseGeneration,
  ]);

  return {
    liveDocId,
    liveRunStatus,
    liveUsage,
    liveMain,
    liveSubagents,
    effectivePendingQuestion,
    mergedResolvedQuestions,
    recordResolvedQuestion,
    clearLivePendingQuestion,
  };
}

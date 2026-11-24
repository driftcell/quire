import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { subscribeSessionEvents } from "@/api/sse";
import type {
  ActiveRunStatus,
  RunTraceSubagent,
  RunTraceSummary,
  SessionDetail,
  SessionEvent,
} from "@/contracts/session-protocol";
import { mergeHydratedSubagents, hydrateActiveRun } from "@/lib/liveTrace";
import {
  clearLivePendingQuestion as clearPendingQuestion,
  emptyLiveSessionState,
  hydrateLiveSession,
  recordResolvedQuestion as applyResolvedQuestion,
  reduceSessionEventBatch,
  resetLiveRun,
  type LiveSessionState,
  type SessionEventEffect,
} from "@/lib/sessionEventReducer";

export type { ResolvedQuestionEntry } from "@/lib/sessionEventReducer";
import type { ResolvedQuestionEntry } from "@/lib/sessionEventReducer";

export function hydrationCursor(
  currentAfterSeq: number,
  lastEventSeq: number,
): number {
  return Math.max(currentAfterSeq, lastEventSeq);
}

export interface UseSessionEventStreamOptions {
  sessionId: string;
  enabled: boolean;
  lastEventSeq: number;
  serverActiveRunStatus?: ActiveRunStatus;
  activeRunTrace?: RunTraceSummary | null;
  activeSubagentTraces?: RunTraceSubagent[];
  serverPendingQuestion?: SessionDetail["pending_question"];
  serverResolvedQuestions?: SessionDetail["resolved_questions"];
}

function applyEffect(
  effect: SessionEventEffect,
  sessionId: string,
  queryClient: ReturnType<typeof useQueryClient>,
  liveStateRef: { current: LiveSessionState },
  setLiveState: (state: LiveSessionState) => void,
  afterSeqRef: { current: number },
  hydratedRunIdRef: { current: string | null },
  setSseGeneration: (update: (generation: number) => number) => void,
) {
  switch (effect.type) {
    case "invalidate": {
      const queryKey =
        effect.target === "session"
          ? ["session", sessionId]
          : effect.target === "traces"
            ? ["session-traces", sessionId]
            : ["sessions"];
      void queryClient.invalidateQueries({ queryKey });
      break;
    }
    case "set-compacting":
      queryClient.setQueryData<SessionDetail>(
        ["session", sessionId],
        (current) =>
          current ? { ...current, compacting: effect.value } : current,
      );
      break;
    case "set-pending-queue":
      queryClient.setQueryData<SessionDetail>(
        ["session", sessionId],
        (current) =>
          current ? { ...current, pending_queue: effect.items } : current,
      );
      break;
    case "set-session-title":
      queryClient.setQueryData<SessionDetail>(
        ["session", sessionId],
        (current) => (current ? { ...current, title: effect.title } : current),
      );
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      break;
    case "set-doc-linked":
      queryClient.setQueryData<SessionDetail>(
        ["session", sessionId],
        (current) =>
          current
            ? {
                ...current,
                doc_id: effect.doc_id,
                linked_doc_id: effect.doc_id,
              }
            : current,
      );
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      break;
    case "set-run-status":
      queryClient.setQueryData<SessionDetail>(
        ["session", sessionId],
        (current) =>
          current ? { ...current, active_run_status: effect.value } : current,
      );
      break;
    case "refetch-and-clear":
      void (async () => {
        try {
          await Promise.all([
            queryClient.refetchQueries({
              queryKey: ["session", sessionId],
            }),
            queryClient.refetchQueries({
              queryKey: ["session-traces", sessionId],
            }),
          ]);
        } catch {
          return;
        }
        const next = resetLiveRun(liveStateRef.current, effect.runId);
        liveStateRef.current = next;
        setLiveState(next);
        hydratedRunIdRef.current = null;
      })();
      break;
    case "sync-required":
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
      break;
  }
}

export function useSessionEventStream({
  sessionId,
  enabled,
  lastEventSeq,
  serverActiveRunStatus,
  activeRunTrace,
  activeSubagentTraces,
  serverPendingQuestion,
  serverResolvedQuestions,
}: UseSessionEventStreamOptions) {
  const queryClient = useQueryClient();
  const [liveState, setLiveState] = useState<LiveSessionState>(() => ({
    ...emptyLiveSessionState(lastEventSeq),
    livePendingQuestion: serverPendingQuestion ?? null,
  }));
  const [sseGeneration, setSseGeneration] = useState(0);

  const liveStateRef = useRef<LiveSessionState>(liveState);
  useEffect(() => {
    liveStateRef.current = liveState;
  }, [liveState]);

  const afterSeqRef = useRef<number>(lastEventSeq);
  const hydratedEventSeqRef = useRef<number>(lastEventSeq);
  useEffect(() => {
    hydratedEventSeqRef.current = lastEventSeq;
  }, [lastEventSeq]);

  const hydratedRunIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (!enabled || !activeRunTrace) {
      return;
    }
    const runId = activeRunTrace.run_id;
    const current = liveStateRef.current.liveMain;
    const isStreaming =
      serverActiveRunStatus === "running" ||
      activeRunTrace.status === "running";
    const afterSeq = hydrationCursor(
      liveStateRef.current.afterSeq,
      hydratedEventSeqRef.current,
    );
    afterSeqRef.current = Math.max(afterSeqRef.current, afterSeq);
    if (current) {
      if (current.runId !== runId || hydratedRunIdRef.current === runId) {
        // A local trace already owns the pane. Never replace it with a detail
        // snapshot that may be behind the SSE cursor, but do synchronize its
        // watermark before the duplicate-frame guard evaluates new events.
        const next = { ...liveStateRef.current, afterSeq };
        liveStateRef.current = next;
        setLiveState(next);
        return;
      }
      const hydrated = hydrateActiveRun(
        activeRunTrace,
        activeSubagentTraces,
        isStreaming,
      );
      const next = {
        ...liveStateRef.current,
        afterSeq,
        liveSubagents: mergeHydratedSubagents(
          liveStateRef.current.liveSubagents,
          hydrated.subagents,
        ),
        liveRunOwnerId: runId,
      };
      liveStateRef.current = next;
      setLiveState(next);
      hydratedRunIdRef.current = runId;
      return;
    }
    const next = {
      ...hydrateLiveSession(
        activeRunTrace,
        activeSubagentTraces,
        isStreaming,
        liveStateRef.current,
      ),
      afterSeq,
    };
    liveStateRef.current = next;
    setLiveState(next);
    hydratedRunIdRef.current = runId;
  }, [enabled, activeRunTrace, activeSubagentTraces, serverActiveRunStatus]);

  const effectivePendingQuestion =
    liveState.livePendingQuestion ?? serverPendingQuestion ?? null;

  const recordResolvedQuestion = useCallback(
    (
      question: NonNullable<SessionDetail["pending_question"]>,
      status: "answered" | "skipped" | "dismissed",
      answers?: Parameters<typeof applyResolvedQuestion>[3],
    ) => {
      const next = applyResolvedQuestion(
        liveStateRef.current,
        question,
        status,
        answers,
      );
      liveStateRef.current = next;
      setLiveState(next);
    },
    [],
  );

  const clearLivePendingQuestion = useCallback(() => {
    const next = clearPendingQuestion(liveStateRef.current);
    liveStateRef.current = next;
    setLiveState(next);
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
    for (const item of liveState.resolvedQuestions) {
      byId.set(item.question.question_id, item);
    }
    return Array.from(byId.values());
  }, [serverResolvedQuestions, liveState.resolvedQuestions]);

  useEffect(() => {
    if (!enabled) return undefined;

    // Adopt a detail cursor only while (re)creating the stream. Later query
    // updates must not rewind or skip frames already queued by this connection.
    afterSeqRef.current = Math.max(
      afterSeqRef.current,
      hydratedEventSeqRef.current,
    );
    const eventQueue: SessionEvent[] = [];
    let flushTimer: number | null = null;
    const flushEvents = () => {
      flushTimer = null;
      if (eventQueue.length === 0) {
        return;
      }
      const events = eventQueue.splice(0, eventQueue.length);
      const result = reduceSessionEventBatch(liveStateRef.current, events);
      liveStateRef.current = result.state;
      setLiveState(result.state);
      afterSeqRef.current = Math.max(
        afterSeqRef.current,
        result.state.afterSeq,
      );
      for (const effect of result.effects) {
        applyEffect(
          effect,
          sessionId,
          queryClient,
          liveStateRef,
          setLiveState,
          afterSeqRef,
          hydratedRunIdRef,
          setSseGeneration,
        );
      }
    };
    const unsubscribe = subscribeSessionEvents(
      sessionId,
      () => afterSeqRef.current,
      {
        onEvent(event) {
          eventQueue.push(event);
          flushTimer ??= window.setTimeout(flushEvents, 16);
        },
      },
    );
    return () => {
      if (flushTimer !== null) {
        window.clearTimeout(flushTimer);
      }
      eventQueue.length = 0;
      unsubscribe();
    };
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- generation intentionally recreates the stream after sync_required
  }, [sessionId, enabled, queryClient, sseGeneration]);

  return {
    liveDocId: liveState.liveDocId,
    liveRunStatus: liveState.liveRunStatus,
    liveUsage: liveState.liveUsage,
    streamingEstimatedOutputTokens: liveState.streamingEstimatedOutputTokens,
    liveMain: liveState.liveMain,
    liveSubagents: liveState.liveSubagents,
    effectivePendingQuestion,
    mergedResolvedQuestions,
    recordResolvedQuestion,
    clearLivePendingQuestion,
  };
}

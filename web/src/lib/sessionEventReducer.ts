import type {
  ActiveRunStatus,
  ContextUsage,
  PendingQueueItem,
  PendingQuestion,
  QuestionAnswer,
  RunTraceSubagent,
  RunTraceSummary,
  SessionEvent,
} from "@/contracts/session-protocol";
import {
  appendSubagentSegment,
  appendTextSegment,
  appendThinkingSegment,
  appendToolSegment,
  emptyLiveTrace,
  ensureLiveTraceStarted,
  finishThinkingSegment,
  finishToolSegment,
  hydrateActiveRun,
  mergeHydratedSubagents,
  type LiveAgentTrace,
  type LiveSubagentRun,
  type ToolFinishPatch,
} from "@/lib/liveTrace";
import {
  isSubagentScope,
  shouldApplySessionUsage,
  shouldResetLiveState,
} from "@/lib/sessionEvents";

export interface ResolvedQuestionEntry {
  question: PendingQuestion;
  status: "answered" | "skipped" | "dismissed";
  answers?: QuestionAnswer[];
}

export interface LiveSessionState {
  liveDocId: string | null;
  liveRunStatus: ActiveRunStatus | null;
  liveUsage: ContextUsage | null;
  liveMain: LiveAgentTrace | null;
  liveSubagents: LiveSubagentRun[];
  livePendingQuestion: PendingQuestion | null;
  resolvedQuestions: ResolvedQuestionEntry[];
  pendingQueue: PendingQueueItem[] | null;
  afterSeq: number;
  liveRunOwnerId: string | null;
}

export type SessionEventEffect =
  | { type: "invalidate"; target: "session" | "sessions" | "traces" }
  | { type: "refetch-and-clear"; runId: string }
  | { type: "set-compacting"; value: boolean }
  | { type: "set-pending-queue"; items: PendingQueueItem[] }
  | { type: "set-session-title"; title: string }
  | { type: "set-doc-linked"; doc_id: string; title: string }
  | { type: "set-run-status"; value: ActiveRunStatus }
  | { type: "sync-required" };

export interface ReduceResult {
  state: LiveSessionState;
  effects: SessionEventEffect[];
}

export function emptyLiveSessionState(afterSeq = 0): LiveSessionState {
  return {
    liveDocId: null,
    liveRunStatus: null,
    liveUsage: null,
    liveMain: null,
    liveSubagents: [],
    livePendingQuestion: null,
    resolvedQuestions: [],
    pendingQueue: null,
    afterSeq,
    liveRunOwnerId: null,
  };
}

function eventTimestampMs(timestamp: string): number {
  const parsed = Date.parse(timestamp);
  return Number.isFinite(parsed) ? parsed : Date.now();
}

function withMainTrace(
  state: LiveSessionState,
  runId: string,
  eventTs: number,
): LiveAgentTrace {
  return ensureLiveTraceStarted(
    state.liveMain ?? emptyLiveTrace("main", runId, eventTs),
    eventTs,
  );
}

function withSubagentTrace(
  trace: LiveAgentTrace,
  eventTs: number,
): LiveAgentTrace {
  return ensureLiveTraceStarted(trace, eventTs);
}

function mapSubagent(
  state: LiveSessionState,
  runId: string,
  update: (subagent: LiveSubagentRun) => LiveSubagentRun,
): LiveSubagentRun[] {
  return state.liveSubagents.map((subagent) =>
    subagent.subagentRunId === runId ? update(subagent) : subagent,
  );
}

function finishMainThinking(
  state: LiveSessionState,
  runId: string,
  eventTs: number,
): LiveAgentTrace {
  return finishThinkingSegment(withMainTrace(state, runId, eventTs), eventTs);
}

function finishAllThinking(
  state: LiveSessionState,
  eventTs: number,
): LiveSessionState {
  return {
    ...state,
    liveMain: state.liveMain
      ? finishThinkingSegment(state.liveMain, eventTs)
      : null,
    liveSubagents: state.liveSubagents.map((subagent) => ({
      ...subagent,
      trace: finishThinkingSegment(subagent.trace, eventTs),
    })),
  };
}

/** Apply a delayed terminal cleanup only while the same run owns the pane. */
export function resetLiveRun(
  state: LiveSessionState,
  terminalRunId: string,
): LiveSessionState {
  if (!shouldResetLiveState(state.liveRunOwnerId, terminalRunId)) {
    return state;
  }
  return {
    ...state,
    liveMain: null,
    liveSubagents: [],
    liveRunOwnerId: null,
  };
}

export function recordResolvedQuestion(
  state: LiveSessionState,
  question: PendingQuestion,
  status: "answered" | "skipped" | "dismissed",
  answers?: QuestionAnswer[],
): LiveSessionState {
  const resolvedQuestions = state.resolvedQuestions.filter(
    (entry) => entry.question.question_id !== question.question_id,
  );
  resolvedQuestions.push({ question, status, answers });
  return { ...state, resolvedQuestions };
}

export function clearLivePendingQuestion(
  state: LiveSessionState,
): LiveSessionState {
  return { ...state, livePendingQuestion: null };
}

/** Hydrate an active run into the same state the live reducer maintains. */
export function hydrateLiveSession(
  trace: RunTraceSummary,
  subagents: RunTraceSubagent[] | undefined,
  isStreaming: boolean,
  previous?: LiveSessionState,
): LiveSessionState {
  const hydrated = hydrateActiveRun(trace, subagents, isStreaming);
  const base = previous ?? emptyLiveSessionState();
  if (base.liveMain) {
    return {
      ...base,
      liveSubagents: mergeHydratedSubagents(
        base.liveSubagents,
        hydrated.subagents,
      ),
      liveRunOwnerId: trace.run_id,
    };
  }
  return {
    ...base,
    liveMain: hydrated.main,
    liveSubagents: hydrated.subagents,
    liveRunOwnerId: trace.run_id,
  };
}

function assertNever(value: never): never {
  throw new Error(`unhandled session event: ${JSON.stringify(value)}`);
}

/** Pure event-to-state projection. The hook only executes the returned effects. */
export function reduceSessionEvent(
  state: LiveSessionState,
  event: SessionEvent,
): ReduceResult {
  // Reconnects and detail hydration may overlap. An already projected frame
  // must never append duplicate text or resurrect a completed tool as running.
  if (
    event.event !== "sync_required" &&
    event.seq > 0 &&
    event.seq <= state.afterSeq
  ) {
    return { state, effects: [] };
  }
  const effects: SessionEventEffect[] = [];
  const eventTs = eventTimestampMs(event.timestamp);
  const isSubagentEvent = isSubagentScope(event);
  const afterSeq =
    event.event !== "sync_required" && event.seq > 0
      ? Math.max(state.afterSeq, event.seq)
      : state.afterSeq;
  let next: LiveSessionState = { ...state, afterSeq };

  switch (event.event) {
    case "doc_linked":
      next = { ...next, liveDocId: event.doc_id };
      effects.push({
        type: "set-doc-linked",
        doc_id: event.doc_id,
        title: event.title,
      });
      break;
    case "session_title_updated":
      effects.push({ type: "set-session-title", title: event.title });
      break;
    case "user_message_added":
      effects.push({ type: "invalidate", target: "session" });
      break;
    case "turn_started":
      next = {
        ...next,
        liveRunStatus: "running",
        liveRunOwnerId: event.run_id,
        liveMain: emptyLiveTrace("main", event.run_id, eventTs),
        liveSubagents: [],
      };
      effects.push({ type: "set-run-status", value: "running" });
      break;
    case "thinking_delta":
      if (isSubagentEvent) {
        next = {
          ...next,
          liveSubagents: mapSubagent(next, event.run_id, (subagent) => ({
            ...subagent,
            trace: appendThinkingSegment(
              withSubagentTrace(subagent.trace, eventTs),
              event.delta,
              eventTs,
            ),
          })),
        };
      } else {
        next = {
          ...next,
          liveMain: appendThinkingSegment(
            withMainTrace(next, event.run_id, eventTs),
            event.delta,
            eventTs,
          ),
        };
      }
      break;
    case "text_delta":
      if (isSubagentEvent) {
        next = {
          ...next,
          liveSubagents: mapSubagent(next, event.run_id, (subagent) => ({
            ...subagent,
            trace: appendTextSegment(
              finishThinkingSegment(
                withSubagentTrace(subagent.trace, eventTs),
                eventTs,
              ),
              event.delta,
            ),
          })),
        };
      } else {
        next = {
          ...next,
          liveMain: appendTextSegment(
            finishMainThinking(next, event.run_id, eventTs),
            event.delta,
          ),
        };
      }
      break;
    case "task_focus": {
      const focus = {
        title: event.title,
        detail: event.detail ?? undefined,
        sectionId: event.section_id ?? undefined,
      };
      if (isSubagentEvent) {
        next = {
          ...next,
          liveSubagents: mapSubagent(next, event.run_id, (subagent) => ({
            ...subagent,
            trace: {
              ...withSubagentTrace(subagent.trace, eventTs),
              taskFocus: focus,
            },
          })),
        };
      } else {
        next = {
          ...next,
          liveMain: {
            ...withMainTrace(next, event.run_id, eventTs),
            taskFocus: focus,
          },
        };
      }
      break;
    }
    case "tool_call_started": {
      const toolItem = {
        toolCallId: event.tool_call_id,
        toolName: event.tool_name,
        status: "running" as const,
        inputPreview: event.input_preview,
        truncated: event.truncated,
      };
      if (isSubagentEvent) {
        next = {
          ...next,
          liveSubagents: mapSubagent(next, event.run_id, (subagent) => ({
            ...subagent,
            trace: appendToolSegment(
              finishThinkingSegment(
                withSubagentTrace(subagent.trace, eventTs),
                eventTs,
              ),
              toolItem,
            ),
          })),
        };
      } else {
        next = {
          ...next,
          liveMain: appendToolSegment(
            finishMainThinking(next, event.run_id, eventTs),
            toolItem,
          ),
        };
      }
      break;
    }
    case "tool_call_finished": {
      const patch: ToolFinishPatch = {
        status:
          event.status === "failed" || event.status === "error"
            ? "failed"
            : event.status === "skipped"
              ? "skipped"
              : "completed",
        outputPreview: event.output_preview,
        durationMs: event.duration_ms,
        truncated: event.truncated,
      };
      if (isSubagentEvent) {
        next = {
          ...next,
          liveSubagents: mapSubagent(next, event.run_id, (subagent) => ({
            ...subagent,
            trace: finishToolSegment(subagent.trace, event.tool_call_id, patch),
          })),
        };
      } else {
        next = {
          ...next,
          liveMain: next.liveMain
            ? finishToolSegment(next.liveMain, event.tool_call_id, patch)
            : null,
        };
      }
      break;
    }
    case "subagent_started": {
      const existing = next.liveSubagents.find(
        (subagent) => subagent.subagentRunId === event.subagent_run_id,
      );
      if (existing) {
        const resumed: LiveSubagentRun = {
          ...existing,
          task: event.task,
          sectionId: event.section_id ?? existing.sectionId,
          displayName: event.display_name ?? existing.displayName,
          status: "running",
          summary: undefined,
          trace: {
            ...finishThinkingSegment(existing.trace, eventTs),
            isStreaming: true,
          },
        };
        next = {
          ...next,
          liveSubagents: [
            ...next.liveSubagents.filter(
              (subagent) => subagent.subagentRunId !== event.subagent_run_id,
            ),
            resumed,
          ],
        };
      } else {
        next = {
          ...next,
          liveSubagents: [
            ...next.liveSubagents,
            {
              subagentRunId: event.subagent_run_id,
              agentName: event.agent_name,
              displayName: event.display_name ?? undefined,
              task: event.task,
              sectionId: event.section_id ?? undefined,
              status: "running",
              trace: emptyLiveTrace(
                event.agent_name,
                event.subagent_run_id,
                eventTs,
              ),
            },
          ],
        };
      }
      next = {
        ...next,
        // Starting or resuming a child settles any trailing main-agent thinking
        // segment and anchors the subagent in the latest execution block.
        liveMain: appendSubagentSegment(
          finishMainThinking(next, event.run_id, eventTs),
          event.subagent_run_id,
        ),
      };
      break;
    }
    case "subagent_finished":
      next = {
        ...next,
        liveSubagents: mapSubagent(next, event.subagent_run_id, (subagent) => ({
          ...subagent,
          status: event.status === "failed" ? "failed" : "completed",
          summary: event.summary,
          trace: {
            ...finishThinkingSegment(subagent.trace, eventTs),
            isStreaming: false,
          },
        })),
      };
      break;
    case "context_usage":
      if (shouldApplySessionUsage(event)) {
        next = {
          ...next,
          liveUsage: {
            input_tokens: event.input_tokens,
            output_tokens: event.output_tokens,
            cached_tokens: event.cached_tokens,
            context_used: event.context_used,
            context_limit: event.context_limit,
          },
        };
      }
      break;
    case "compaction_started":
      effects.push({ type: "set-compacting", value: true });
      break;
    case "compaction_finished":
      effects.push({ type: "set-compacting", value: false });
      effects.push({ type: "invalidate", target: "session" });
      break;
    case "questionnaire_requested":
      next = {
        ...next,
        liveRunStatus: "awaiting_input",
        liveMain: next.liveMain
          ? finishThinkingSegment(next.liveMain, eventTs)
          : null,
        livePendingQuestion: {
          question_id: event.question_id,
          run_id: event.run_id,
          tool_call_id: event.tool_call_id,
          questions: event.questions,
          created_at: event.timestamp,
        },
      };
      break;
    case "questionnaire_answered": {
      const target = next.livePendingQuestion;
      if (target && target.question_id === event.question_id) {
        next = recordResolvedQuestion(
          next,
          target,
          event.skipped_all ? "skipped" : "answered",
          event.answers,
        );
      }
      next = {
        ...next,
        livePendingQuestion:
          next.livePendingQuestion?.question_id === event.question_id
            ? null
            : next.livePendingQuestion,
      };
      effects.push({ type: "invalidate", target: "session" });
      effects.push({ type: "invalidate", target: "traces" });
      break;
    }
    case "questionnaire_dismissed": {
      const target = next.livePendingQuestion;
      if (target && target.question_id === event.question_id) {
        next = recordResolvedQuestion(next, target, "dismissed");
      }
      next = {
        ...next,
        livePendingQuestion:
          next.livePendingQuestion?.question_id === event.question_id
            ? null
            : next.livePendingQuestion,
      };
      effects.push({ type: "invalidate", target: "session" });
      effects.push({ type: "invalidate", target: "traces" });
      break;
    }
    case "turn_finished":
      next = {
        ...next,
        ...finishAllThinking(next, eventTs),
        liveRunStatus: "idle",
      };
      effects.push({ type: "refetch-and-clear", runId: event.run_id });
      effects.push({ type: "invalidate", target: "sessions" });
      break;
    case "turn_superseded":
      next = {
        ...next,
        ...finishAllThinking(next, eventTs),
        liveRunStatus: "idle",
      };
      effects.push({ type: "refetch-and-clear", runId: event.run_id });
      effects.push({ type: "invalidate", target: "traces" });
      effects.push({ type: "invalidate", target: "sessions" });
      break;
    case "turn_failed":
      next = {
        ...next,
        ...finishAllThinking(next, eventTs),
        liveRunStatus: event.recoverable ? "interrupted" : "failed",
      };
      next = {
        ...next,
        liveMain: next.liveMain
          ? { ...next.liveMain, isStreaming: false }
          : null,
      };
      effects.push({ type: "invalidate", target: "session" });
      break;
    case "sync_required":
      effects.push({ type: "sync-required" });
      break;
    case "pending_queue_updated":
      next = { ...next, pendingQueue: event.items };
      effects.push({ type: "set-pending-queue", items: event.items });
      break;
    default:
      return assertNever(event);
  }

  return { state: next, effects };
}

/** Project one network batch with a single React state commit at the caller. */
export function reduceSessionEventBatch(
  state: LiveSessionState,
  events: SessionEvent[],
): ReduceResult {
  let next = state;
  const effects: SessionEventEffect[] = [];
  for (const event of events) {
    const result = reduceSessionEvent(next, event);
    next = result.state;
    effects.push(...result.effects);
  }
  return { state: next, effects };
}

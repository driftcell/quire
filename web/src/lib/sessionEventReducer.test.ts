import { describe, expect, test } from "bun:test";
import mainRun from "../../../contracts/fixtures/session-events/main_run.json";
import subagentRun from "../../../contracts/fixtures/session-events/subagent_run.json";
import subagentMultiRound from "../../../contracts/fixtures/session-events/subagent_multi_round.json";
import inlineUserMessage from "../../../contracts/fixtures/session-events/inline_user_message.json";
import sessionControlFrames from "../../../contracts/fixtures/session-events/session_control_frames.json";
import type {
  RunTraceSummary,
  SessionEvent,
} from "@/contracts/session-protocol";
import { isRecord } from "@/lib/guards";
import {
  emptyLiveSessionState,
  hydrateLiveSession,
  reduceSessionEvent,
  resetLiveRun as resetLiveRunForTest,
  type LiveSessionState,
} from "./sessionEventReducer";

interface TraceFixture {
  events: SessionEvent[];
  expected_traces: RunTraceSummary[];
  prefix_traces: RunTraceSummary[][];
}

function isTraceFixture(value: unknown): value is TraceFixture {
  return (
    isRecord(value) &&
    Array.isArray(value.events) &&
    Array.isArray(value.expected_traces) &&
    Array.isArray(value.prefix_traces)
  );
}

function parseFixture(value: unknown): TraceFixture {
  if (!isTraceFixture(value)) {
    throw new Error("invalid session event fixture");
  }
  return value;
}

const fixtures: Record<string, TraceFixture> = {
  mainRun: parseFixture(mainRun),
  subagentRun: parseFixture(subagentRun),
  subagentMultiRound: parseFixture(subagentMultiRound),
  inlineUserMessage: parseFixture(inlineUserMessage),
  sessionControlFrames: parseFixture(sessionControlFrames),
};

function reduceAll(events: SessionEvent[]): LiveSessionState {
  return events.reduce(
    (state, event) => reduceSessionEvent(state, event).state,
    emptyLiveSessionState(),
  );
}

function comparable(state: LiveSessionState) {
  return {
    liveDocId: state.liveDocId,
    liveRunStatus: state.liveRunStatus,
    liveUsage: state.liveUsage,
    liveMain: state.liveMain,
    liveSubagents: state.liveSubagents,
    livePendingQuestion: state.livePendingQuestion,
    resolvedQuestions: state.resolvedQuestions,
    pendingQueue: state.pendingQueue,
    afterSeq: state.afterSeq,
    liveRunOwnerId: state.liveRunOwnerId,
  };
}

describe("prefix/suffix hydration equivalence", () => {
  for (const [name, fixture] of Object.entries(fixtures)) {
    test(name, () => {
      const expected = reduceAll(fixture.events);
      for (let split = 0; split < fixture.events.length; split += 1) {
        const prefixState = reduceAll(fixture.events.slice(0, split));
        const active = fixture.prefix_traces[split]?.at(-1);
        const initial =
          active && active.status === "running"
            ? hydrateLiveSession(active, undefined, true, {
                ...prefixState,
                liveMain: null,
                liveSubagents: [],
                liveRunOwnerId: null,
              })
            : prefixState;
        const actual = fixture.events
          .slice(split)
          .reduce(
            (state, event) => reduceSessionEvent(state, event).state,
            initial,
          );
        expect(comparable(actual)).toEqual(comparable(expected));
      }
    });
  }
});

describe("run lifecycle guards", () => {
  const turnStarted: SessionEvent = {
    seq: 1,
    run_id: "run_1",
    agent_id: "main",
    parent_run_id: null,
    timestamp: "2026-01-01T00:00:00Z",
    event: "turn_started",
  };

  test("a late terminal reset does not clear a newer run", () => {
    const first = reduceAll([turnStarted]);
    expect(resetLiveRunForTest(first, "run_1").liveMain).toBeNull();

    const newer = reduceSessionEvent(first, {
      ...turnStarted,
      seq: 2,
      run_id: "run_2",
    }).state;
    const preserved = resetLiveRunForTest(newer, "run_1");
    expect(preserved.liveMain?.runId).toBe("run_2");
  });

  test("turn_failed stops the main streaming marker", () => {
    const running = reduceAll([
      turnStarted,
      {
        seq: 2,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:01Z",
        event: "thinking_delta",
        delta: "plan",
      },
    ]);
    const failed = reduceSessionEvent(running, {
      seq: 3,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:02Z",
      event: "turn_failed",
      error: "interrupted",
      recoverable: true,
    }).state;
    expect(failed.liveMain?.isStreaming).toBe(false);
  });

  test("subagent usage never updates the session meter", () => {
    const usage: SessionEvent = {
      seq: 2,
      run_id: "run_sub_1",
      agent_id: "alpha",
      parent_run_id: "run_1",
      timestamp: "2026-01-01T00:00:01Z",
      event: "context_usage",
      input_tokens: 10,
      output_tokens: 5,
      cached_tokens: 0,
      context_used: 15,
      context_limit: 100,
    };
    expect(
      reduceSessionEvent(emptyLiveSessionState(), usage).state.liveUsage,
    ).toBeNull();
    const mainUsage = reduceSessionEvent(emptyLiveSessionState(), {
      ...usage,
      agent_id: "main",
      parent_run_id: null,
      run_id: "run_1",
    }).state.liveUsage;
    expect(mainUsage?.context_used).toBe(15);
  });
});

describe("refetch-reducing effects", () => {
  test("session_title_updated updates the cache without a detail refetch", () => {
    const { effects } = reduceSessionEvent(emptyLiveSessionState(), {
      seq: 1,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:00Z",
      event: "session_title_updated",
      title: "Report",
    });
    expect(effects).toEqual([{ type: "set-session-title", title: "Report" }]);
  });

  test("pending_queue_updated only patches the pending queue", () => {
    const { effects } = reduceSessionEvent(emptyLiveSessionState(), {
      seq: 1,
      run_id: "queue",
      agent_id: "system",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:00Z",
      event: "pending_queue_updated",
      items: [],
    });
    expect(effects).toEqual([{ type: "set-pending-queue", items: [] }]);
  });

  test("turn_started patches active_run_status instead of refetching", () => {
    const { effects } = reduceSessionEvent(emptyLiveSessionState(), {
      seq: 1,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:00Z",
      event: "turn_started",
    });
    expect(effects).toEqual([{ type: "set-run-status", value: "running" }]);
  });

  test("doc_linked patches the bound document", () => {
    const { effects } = reduceSessionEvent(emptyLiveSessionState(), {
      seq: 1,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:00Z",
      event: "doc_linked",
      doc_id: "doc_1",
      title: "Report",
    });
    expect(effects).toEqual([
      { type: "set-doc-linked", doc_id: "doc_1", title: "Report" },
    ]);
  });
});

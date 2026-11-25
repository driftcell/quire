import { describe, expect, test } from "bun:test";
import mainRun from "../../../contracts/fixtures/session-events/main_run.json";
import subagentRun from "../../../contracts/fixtures/session-events/subagent_run.json";
import subagentResume from "../../../contracts/fixtures/session-events/subagent_resume.json";
import subagentMultiRound from "../../../contracts/fixtures/session-events/subagent_multi_round.json";
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
  reduceSessionEventBatch,
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

const mainRunEvents = parseFixture(mainRun).events;
const fixtures: Record<string, TraceFixture> = {
  mainRun: parseFixture(mainRun),
  subagentRun: parseFixture(subagentRun),
  subagentMultiRound: parseFixture(subagentMultiRound),
  subagentResume: parseFixture(subagentResume),
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

  test("turn_superseded ends the current run and requests trace refresh", () => {
    const running = reduceAll([turnStarted]);
    const result = reduceSessionEvent(running, {
      seq: 2,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:01Z",
      event: "turn_superseded",
    });
    expect(result.state.liveRunStatus).toBe("idle");
    expect(result.effects).toContainEqual({
      type: "refetch-and-clear",
      runId: "run_1",
    });
    expect(result.effects).toContainEqual({
      type: "invalidate",
      target: "traces",
    });
  });

  test("skipped sibling tools retain a terminal skipped status", () => {
    const running = reduceAll([
      turnStarted,
      {
        seq: 2,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:01Z",
        event: "tool_call_started",
        tool_call_id: "call_1",
        tool_name: "apply_operations",
        input_preview: "{}",
        truncated: false,
      },
    ]);
    const finished = reduceSessionEvent(running, {
      seq: 3,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:02Z",
      event: "tool_call_finished",
      tool_call_id: "call_1",
      status: "skipped",
      output_preview: "{}",
      duration_ms: 0,
      truncated: false,
    }).state;
    const segment = finished.liveMain?.segments.at(-1);
    expect(segment?.kind).toBe("execution");
    if (segment?.kind === "execution") {
      expect(segment.tools[0]?.status).toBe("skipped");
    }
  });


  test("streaming deltas accumulate estimated output tokens and reconcile on context_usage", () => {
    let state = reduceSessionEvent(emptyLiveSessionState(), {
      seq: 1,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:00Z",
      event: "turn_started",
    }).state;
    expect(state.streamingEstimatedOutputTokens).toBe(0);

    state = reduceSessionEvent(state, {
      seq: 2,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:01Z",
      event: "thinking_delta",
      delta: "正在分析高校学科建设数据",
    }).state;
    expect(state.streamingEstimatedOutputTokens).toBeGreaterThan(5);

    const afterThinking = state.streamingEstimatedOutputTokens;
    state = reduceSessionEvent(state, {
      seq: 3,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:02Z",
      event: "text_delta",
      delta: "hello world",
    }).state;
    expect(state.streamingEstimatedOutputTokens).toBeGreaterThan(afterThinking);

    state = reduceSessionEvent(state, {
      seq: 4,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:03Z",
      event: "context_usage",
      input_tokens: 100,
      output_tokens: 24,
      cached_tokens: 0,
      context_used: 124,
      context_limit: 128000,
      cost_rmb: 0.0012,
      output_price_rmb: 8.0,
    }).state;
    expect(state.streamingEstimatedOutputTokens).toBe(0);
    expect(state.liveUsage?.cost_rmb).toBe(0.0012);
    expect(state.liveUsage?.output_price_rmb).toBe(8.0);
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
    const mainState = reduceSessionEvent(emptyLiveSessionState(), {
      ...usage,
      agent_id: "main",
      parent_run_id: null,
      run_id: "run_1",
      cost_rmb: 0.0025,
    }).state;
    expect(mainState.liveUsage?.context_used).toBe(15);
    expect(mainState.liveUsage?.cost_rmb).toBe(0.0025);

    const afterSubagentCost = reduceSessionEvent(mainState, {
      ...usage,
      seq: 3,
      context_used: 999,
      context_limit: 5000,
      cost_rmb: 0.0042,
    }).state.liveUsage;
    expect(afterSubagentCost?.context_used).toBe(15);
    expect(afterSubagentCost?.context_limit).toBe(100);
    expect(afterSubagentCost?.cost_rmb).toBe(0.0042);
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

describe("batched replay protection", () => {
  test("a replayed tool start cannot resurrect a completed call", () => {
    const final = reduceAll(mainRunEvents);
    const started = mainRunEvents.find(
      (event) => event.event === "tool_call_started",
    );
    expect(started).toBeDefined();
    if (!started) return;

    const replayed = reduceSessionEvent(final, started);
    expect(comparable(replayed.state)).toEqual(comparable(final));
    expect(replayed.effects).toEqual([]);
  });

  test("a batch matches sequential projection without duplicate text", () => {
    const running = reduceAll(mainRunEvents.slice(0, 2));
    const first = mainRunEvents.find((event) => event.event === "text_delta");
    expect(first).toBeDefined();
    if (!first || first.event !== "text_delta") return;
    const second = {
      ...first,
      seq: first.seq + 1,
      delta: "next",
    };
    const withFirst = reduceSessionEvent(running, first).state;
    const expected = reduceSessionEvent(withFirst, second).state;
    const actual = reduceSessionEventBatch(running, [first, first, second]);

    expect(comparable(actual.state)).toEqual(comparable(expected));
  });
});

describe("task_focus and todos_updated independence", () => {
  test("keeps live work status and dynamic todo checklist orthogonal for main and subagents", () => {
    const state = reduceAll([
      {
        seq: 1,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:00Z",
        event: "turn_started",
      },
      {
        seq: 2,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:01Z",
        event: "todos_updated",
        todos: [
          { title: "立项与并行调研", status: "in_progress" },
          { title: "汇总定调与建桩", status: "pending" },
        ],
      },
      {
        seq: 3,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:02Z",
        event: "task_focus",
        title: "核验国自然立项口径冲突",
        detail: "比对2021—2025年面上与青年增量",
        section_id: "sec_1",
      },
      {
        seq: 4,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:03Z",
        event: "todos_updated",
        todos: [
          { title: "立项与并行调研", status: "completed" },
          { title: "补充交叉核验", status: "in_progress" },
          { title: "汇总定调与建桩", status: "pending" },
        ],
      },
      {
        seq: 5,
        run_id: "sub_1",
        agent_id: "researcher",
        parent_run_id: "run_1",
        timestamp: "2026-01-01T00:00:04Z",
        event: "subagent_started",
        subagent_run_id: "sub_1",
        agent_name: "researcher",
        task: "交叉核验国自然立项",
      },
      {
        seq: 6,
        run_id: "sub_1",
        agent_id: "researcher",
        parent_run_id: "run_1",
        timestamp: "2026-01-01T00:00:05Z",
        event: "todos_updated",
        todos: [{ title: "查核基金委口径", status: "in_progress" }],
      },
    ]);

    expect(state.liveMain?.taskFocus?.title).toBe("核验国自然立项口径冲突");
    expect(state.liveMain?.taskFocus?.detail).toBe(
      "比对2021—2025年面上与青年增量",
    );
    expect(state.liveMain?.taskFocus?.sectionId).toBe("sec_1");
    expect(state.liveMain?.taskFocus?.todos).toHaveLength(3);
    expect(state.liveMain?.taskFocus?.todos?.[1]?.title).toBe("补充交叉核验");
    expect(state.liveMain?.taskFocus?.todos?.[1]?.status).toBe("in_progress");

    const sub = state.liveSubagents[0];
    expect(sub?.trace.taskFocus?.title).toBe("");
    expect(sub?.trace.taskFocus?.todos).toHaveLength(1);
  });
});

describe("decoupled questionnaire lifecycle", () => {
  test("turn_finished after questionnaire_requested preserves awaiting_input status until answered", () => {
    const requested = reduceAll([
      {
        seq: 1,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:00Z",
        event: "turn_started",
      },
      {
        seq: 2,
        run_id: "run_1",
        agent_id: "main",
        parent_run_id: null,
        timestamp: "2026-01-01T00:00:01Z",
        event: "questionnaire_requested",
        question_id: "q_1",
        tool_call_id: "tc_ask",
        questions: [
          {
            id: "scope",
            title: "选择对照范围",
            options: [{ id: "c9", label: "C9 高校" }],
          },
        ],
      },
    ]);
    expect(requested.liveRunStatus).toBe("awaiting_input");
    expect(requested.livePendingQuestion?.question_id).toBe("q_1");

    const closed = reduceSessionEvent(requested, {
      seq: 3,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:02Z",
      event: "turn_finished",
    });
    expect(closed.state.liveRunStatus).toBe("awaiting_input");
    expect(closed.state.livePendingQuestion?.question_id).toBe("q_1");
    expect(closed.effects).toContainEqual({
      type: "refetch-and-clear",
      runId: "run_1",
    });

    const cleared = resetLiveRunForTest(closed.state, "run_1");
    expect(cleared.liveMain).toBeNull();
    expect(cleared.livePendingQuestion?.question_id).toBe("q_1");

    const answered = reduceSessionEvent(cleared, {
      seq: 4,
      run_id: "run_1",
      agent_id: "main",
      parent_run_id: null,
      timestamp: "2026-01-01T00:00:03Z",
      event: "questionnaire_answered",
      question_id: "q_1",
      answers: [{ question_id: "scope", selected_option_ids: ["c9"] }],
      skipped_all: false,
    });
    expect(answered.state.livePendingQuestion).toBeNull();
    expect(answered.state.resolvedQuestions).toHaveLength(1);
    expect(answered.state.resolvedQuestions[0]?.status).toBe("answered");
  });
});

import { describe, expect, test } from "bun:test";
import {
  appendTextSegment,
  appendThinkingSegment,
  emptyLiveTrace,
  finishThinkingSegment,
} from "./liveTrace";

describe("live thinking timestamps", () => {
  test("keeps the server start and settles the active thinking duration", () => {
    let trace = emptyLiveTrace("main");
    trace = appendThinkingSegment(trace, "first", 1_000);
    trace = appendThinkingSegment(trace, " second", 1_500);

    const first = trace.segments[0];
    expect(first?.kind).toBe("thinking");
    if (first?.kind !== "thinking") {
      throw new Error("expected thinking segment");
    }
    expect(first.startedAtMs).toBe(1_000);
    expect(first.text).toBe("first second");

    trace = finishThinkingSegment(trace, 4_000);
    const settled = trace.segments[0];
    if (settled?.kind !== "thinking") {
      throw new Error("expected thinking segment");
    }
    expect(settled.durationMs).toBe(3_000);

    trace = appendThinkingSegment(trace, "third", 5_000);
    expect(trace.segments).toHaveLength(2);
    const second = trace.segments[1];
    if (second?.kind !== "thinking") {
      throw new Error("expected second thinking segment");
    }
    expect(second.startedAtMs).toBe(5_000);
  });

  test("ignores settlement after text has replaced the active segment", () => {
    let trace = emptyLiveTrace("main");
    trace = appendThinkingSegment(trace, "plan", 1_000);
    trace = appendTextSegment(finishThinkingSegment(trace, 2_000), "answer");
    trace = finishThinkingSegment(trace, 9_000);

    const text = trace.segments[1];
    expect(text?.kind).toBe("text");
    const thinking = trace.segments[0];
    if (thinking?.kind !== "thinking") {
      throw new Error("expected thinking segment");
    }
    expect(thinking.durationMs).toBe(1_000);
  });
});

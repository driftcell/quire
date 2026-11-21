import { describe, expect, test } from "bun:test";
import {
  isSubagentScope,
  shouldApplySessionUsage,
  shouldResetLiveState,
} from "./sessionEvents";

describe("session event scope", () => {
  test("main frames stay in the main scope", () => {
    expect(isSubagentScope({ agent_id: "main", parent_run_id: null })).toBe(
      false,
    );
    expect(shouldApplySessionUsage({ agent_id: "main" })).toBe(true);
  });

  test("subagent frames are excluded from the session usage meter", () => {
    const child = { agent_id: "alpha", parent_run_id: "run_main" };
    expect(isSubagentScope(child)).toBe(true);
    expect(shouldApplySessionUsage(child)).toBe(false);
    expect(isSubagentScope({ agent_id: "alpha", parent_run_id: null })).toBe(
      true,
    );
  });

  test("an empty agent id does not create a subagent scope", () => {
    expect(isSubagentScope({ agent_id: "", parent_run_id: null })).toBe(false);
  });
});

describe("terminal live-state cleanup", () => {
  test("allows cleanup when the same run owns the pane", () => {
    expect(shouldResetLiveState("run_1", "run_1")).toBe(true);
  });

  test("allows cleanup when no run ownership is known", () => {
    expect(shouldResetLiveState(null, "run_1")).toBe(true);
  });

  test("rejects a late cleanup after a newer run takes over", () => {
    expect(shouldResetLiveState("run_2", "run_1")).toBe(false);
  });
});

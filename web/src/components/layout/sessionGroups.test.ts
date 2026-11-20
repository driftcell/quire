import { describe, expect, test } from "bun:test";
import type { SessionSummary } from "@/contracts/session-protocol";
import { groupSessionsForSidebar } from "./sessionGroups";

function session(
  sessionId: string,
  options: {
    updatedAt: string;
    pinnedAt?: string | null;
    createdAt?: string;
  },
): SessionSummary {
  return {
    session_id: sessionId,
    title: sessionId,
    status: "active",
    active_run_status: "idle",
    last_event_seq: 0,
    context_usage: {
      input_tokens: 0,
      output_tokens: 0,
      cached_tokens: 0,
      context_used: 0,
      context_limit: 0,
    },
    compacting: false,
    compacted_at: null,
    pinned_at: options.pinnedAt ?? null,
    created_at: options.createdAt ?? options.updatedAt,
    updated_at: options.updatedAt,
  };
}

describe("groupSessionsForSidebar", () => {
  test("puts pinned sessions first ordered by pinned_at then recency", () => {
    const now = new Date().toISOString();
    const groups = groupSessionsForSidebar([
      session("old-pin", {
        updatedAt: now,
        pinnedAt: "2026-11-18T08:00:00.000Z",
      }),
      session("new-pin-b", {
        updatedAt: "2026-11-19T09:00:00.000Z",
        pinnedAt: "2026-11-20T08:00:00.000Z",
      }),
      session("new-pin-a", {
        updatedAt: "2026-11-19T10:00:00.000Z",
        pinnedAt: "2026-11-20T08:00:00.000Z",
      }),
      session("unpinned", { updatedAt: now }),
    ]);

    expect(groups[0]?.key).toBe("pinned");
    expect(groups[0]?.label).toBe("置顶");
    expect(groups[0]?.items.map((item) => item.session_id)).toEqual([
      "new-pin-a",
      "new-pin-b",
      "old-pin",
    ]);
    expect(
      groups
        .flatMap((group) => group.items)
        .filter((item) => item.session_id === "unpinned"),
    ).toHaveLength(1);
  });

  test("omits the pinned group entirely when nothing is pinned", () => {
    const groups = groupSessionsForSidebar([
      session("today", { updatedAt: new Date().toISOString() }),
    ]);

    expect(groups.some((group) => group.key === "pinned")).toBe(false);
    expect(groups[0]?.key).toBe("today");
  });

  test("returns an unpinned session to its original date group", () => {
    const today = new Date().toISOString();
    const before = groupSessionsForSidebar([
      session("pinned", {
        updatedAt: today,
        pinnedAt: "2026-11-20T08:00:00.000Z",
      }),
    ]);
    expect(before[0]?.key).toBe("pinned");

    const after = groupSessionsForSidebar([
      session("unpinned", { updatedAt: today }),
    ]);
    expect(after.some((group) => group.key === "pinned")).toBe(false);
    expect(
      after.find((group) => group.key === "today")?.items[0]?.session_id,
    ).toBe("unpinned");
  });
});

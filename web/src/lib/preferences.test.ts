import { beforeEach, describe, expect, test } from "bun:test";
import {
  getPreferencesSnapshot,
  subscribePreferences,
  updatePreferences,
} from "./preferences";

function resetPreferences(): void {
  updatePreferences({
    leftSidebarOpen: true,
    rightSidebarOpen: true,
    reportViewMode: "continuous",
    rightPanelPct: 34,
    defaultStylePresetId: "quire-folio",
  });
}

describe("preferences store", () => {
  beforeEach(resetPreferences);

  test("patches merge into the latest snapshot without overwriting each other", () => {
    updatePreferences({ leftSidebarOpen: false });
    updatePreferences({ reportViewMode: "paged" });
    updatePreferences({ rightSidebarOpen: false });
    updatePreferences({ rightPanelPct: 46 });

    expect(getPreferencesSnapshot()).toEqual({
      leftSidebarOpen: false,
      rightSidebarOpen: false,
      reportViewMode: "paged",
      rightPanelPct: 46,
      defaultStylePresetId: "quire-folio",
    });
  });

  test("subscribers are notified once per effective change", () => {
    let calls = 0;
    const unsubscribe = subscribePreferences(() => {
      calls += 1;
    });

    updatePreferences({ leftSidebarOpen: false });
    updatePreferences({ leftSidebarOpen: false });
    expect(calls).toBe(1);

    unsubscribe();
    updatePreferences({ leftSidebarOpen: true });
    expect(calls).toBe(1);
  });
});

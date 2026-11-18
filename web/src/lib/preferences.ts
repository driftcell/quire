import { isRecord } from "@/lib/guards";

export type ReportViewMode = "continuous" | "paged";

export interface UiPreferences {
  leftSidebarOpen: boolean;
  rightSidebarOpen: boolean;
  reportViewMode: ReportViewMode;
  rightPanelPct: number;
}

const STORAGE_KEY = "quire.ui.preferences.v1";

const DEFAULT_PREFS: UiPreferences = {
  leftSidebarOpen: true,
  rightSidebarOpen: true,
  reportViewMode: "continuous",
  rightPanelPct: 34,
};

export function loadPreferences(): UiPreferences {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return DEFAULT_PREFS;
    }
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) {
      return DEFAULT_PREFS;
    }
    const record = parsed;
    return {
      leftSidebarOpen:
        typeof record.leftSidebarOpen === "boolean"
          ? record.leftSidebarOpen
          : DEFAULT_PREFS.leftSidebarOpen,
      rightSidebarOpen:
        typeof record.rightSidebarOpen === "boolean"
          ? record.rightSidebarOpen
          : DEFAULT_PREFS.rightSidebarOpen,
      reportViewMode:
        record.reportViewMode === "paged" ||
        record.reportViewMode === "continuous"
          ? record.reportViewMode
          : DEFAULT_PREFS.reportViewMode,
      rightPanelPct:
        typeof record.rightPanelPct === "number" &&
        record.rightPanelPct >= 20 &&
        record.rightPanelPct <= 60
          ? record.rightPanelPct
          : DEFAULT_PREFS.rightPanelPct,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function savePreferences(next: UiPreferences): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Ignore storage quota / privacy mode errors.
  }
}

import { useSyncExternalStore } from "react";
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

function parsePreferences(raw: string | null): UiPreferences {
  if (!raw) {
    return DEFAULT_PREFS;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) {
      return DEFAULT_PREFS;
    }
    return {
      leftSidebarOpen:
        typeof parsed.leftSidebarOpen === "boolean"
          ? parsed.leftSidebarOpen
          : DEFAULT_PREFS.leftSidebarOpen,
      rightSidebarOpen:
        typeof parsed.rightSidebarOpen === "boolean"
          ? parsed.rightSidebarOpen
          : DEFAULT_PREFS.rightSidebarOpen,
      reportViewMode:
        parsed.reportViewMode === "paged" ||
        parsed.reportViewMode === "continuous"
          ? parsed.reportViewMode
          : DEFAULT_PREFS.reportViewMode,
      rightPanelPct:
        typeof parsed.rightPanelPct === "number" &&
        parsed.rightPanelPct >= 20 &&
        parsed.rightPanelPct <= 60
          ? parsed.rightPanelPct
          : DEFAULT_PREFS.rightPanelPct,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

function readStoredPreferences(): UiPreferences {
  if (typeof window === "undefined") {
    return DEFAULT_PREFS;
  }
  try {
    return parsePreferences(window.localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_PREFS;
  }
}

function preferencesEqual(left: UiPreferences, right: UiPreferences): boolean {
  return (
    left.leftSidebarOpen === right.leftSidebarOpen &&
    left.rightSidebarOpen === right.rightSidebarOpen &&
    left.reportViewMode === right.reportViewMode &&
    left.rightPanelPct === right.rightPanelPct
  );
}

let currentPrefs = readStoredPreferences();
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

function persistPreferences(preferences: UiPreferences): void {
  if (typeof window === "undefined") {
    return;
  }
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // Ignore storage quota / privacy mode errors.
  }
}

export function getPreferencesSnapshot(): UiPreferences {
  return currentPrefs;
}

export function subscribePreferences(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Merge a patch into the latest snapshot, persist it and notify every
 * subscriber. This is the only supported write path for UI preferences.
 */
export function updatePreferences(patch: Partial<UiPreferences>): void {
  const next = { ...currentPrefs, ...patch };
  if (preferencesEqual(currentPrefs, next)) {
    return;
  }
  currentPrefs = next;
  persistPreferences(next);
  notifyListeners();
}

function handleStorageEvent(event: StorageEvent): void {
  if (event.key !== null && event.key !== STORAGE_KEY) {
    return;
  }
  const next = parsePreferences(event.newValue);
  if (preferencesEqual(currentPrefs, next)) {
    return;
  }
  currentPrefs = next;
  notifyListeners();
}

if (typeof window !== "undefined") {
  window.addEventListener("storage", handleStorageEvent);
}

export function useUiPreferences(): UiPreferences {
  return useSyncExternalStore(
    subscribePreferences,
    getPreferencesSnapshot,
    getPreferencesSnapshot,
  );
}

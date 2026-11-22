import { useCallback } from "react";
import {
  updatePreferences,
  useUiPreferences,
  type UiPreferences,
} from "@/lib/preferences";

export function useWorkspacePreferences() {
  const prefs = useUiPreferences();
  const updatePrefs = useCallback((patch: Partial<UiPreferences>) => {
    updatePreferences(patch);
  }, []);

  const toggleRightSidebar = () => {
    updatePrefs({ rightSidebarOpen: !prefs.rightSidebarOpen });
  };

  return {
    prefs,
    updatePrefs,
    toggleRightSidebar,
  };
}

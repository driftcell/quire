import {
  startTransition,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { usePanelRef } from "react-resizable-panels";
import { loadPreferences, savePreferences } from "@/lib/preferences";

export function useWorkspacePreferences() {
  const [prefs, setPrefs] = useState(() => loadPreferences());
  const updatePrefs = useCallback((patch: Partial<typeof prefs>) => {
    startTransition(() => {
      setPrefs((prev) => {
        const next = { ...prev, ...patch };
        savePreferences(next);
        return next;
      });
    });
  }, []);

  const [rightPanelAnimating, setRightPanelAnimating] = useState(false);
  const rightPanelRef = usePanelRef();
  const rightPanelWasOpenRef = useRef(prefs.rightSidebarOpen);

  useEffect(() => {
    if (!rightPanelAnimating) {
      return undefined;
    }
    const timer = window.setTimeout(() => setRightPanelAnimating(false), 350);
    return () => window.clearTimeout(timer);
  }, [rightPanelAnimating]);

  useEffect(() => {
    const panel = rightPanelRef.current;
    if (!panel) {
      return;
    }
    if (prefs.rightSidebarOpen) {
      if (!rightPanelWasOpenRef.current) {
        panel.resize(`${prefs.rightPanelPct}%`);
      }
    } else if (rightPanelWasOpenRef.current) {
      panel.collapse();
    }
    rightPanelWasOpenRef.current = prefs.rightSidebarOpen;
  }, [prefs.rightSidebarOpen, prefs.rightPanelPct, rightPanelRef]);

  const toggleRightSidebar = () => {
    setRightPanelAnimating(true);
    updatePrefs({ rightSidebarOpen: !prefs.rightSidebarOpen });
  };

  return {
    prefs,
    updatePrefs,
    rightPanelAnimating,
    rightPanelRef,
    toggleRightSidebar,
  };
}

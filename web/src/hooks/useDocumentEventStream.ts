import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { subscribeDocumentEvents } from "@/api/sse";
import type { EphemeralPreviewEvent } from "@/contracts/report-protocol";

export interface UseDocumentEventStreamOptions {
  docId: string | null;
  enabled: boolean;
  currentRev: number;
}

export function useDocumentEventStream({
  docId,
  enabled,
  currentRev,
}: UseDocumentEventStreamOptions) {
  const queryClient = useQueryClient();
  const [ephemeralPreviews, setEphemeralPreviews] = useState<
    Map<string, EphemeralPreviewEvent>
  >(() => new Map());
  const [activeActorsBySection, setActiveActorsBySection] = useState<
    Map<string, string[]>
  >(() => new Map());
  const previewTimersRef = useRef<Map<string, number>>(new Map());
  const presenceTimersRef = useRef<Map<string, number>>(new Map());
  const pendingClearedByRevRef = useRef<Map<number, string[]>>(new Map());
  const currentRevRef = useRef<number>(0);

  useEffect(() => {
    currentRevRef.current = currentRev;
    if (pendingClearedByRevRef.current.size === 0) return;
    const toClear: string[] = [];
    for (const [rev, ids] of pendingClearedByRevRef.current.entries()) {
      if (rev <= currentRev) {
        toClear.push(...ids);
        pendingClearedByRevRef.current.delete(rev);
      }
    }
    if (toClear.length > 0) {
      setEphemeralPreviews((prev) => {
        const next = new Map(prev);
        for (const id of toClear) {
          next.delete(id);
          const timer = previewTimersRef.current.get(id);
          if (timer !== undefined) {
            window.clearTimeout(timer);
            previewTimersRef.current.delete(id);
          }
        }
        return next;
      });
    }
  }, [currentRev]);

  useEffect(() => {
    if (!docId || !enabled) return undefined;
    const timers = previewTimersRef.current;
    const presenceTimers = presenceTimersRef.current;

    const clearPreviewIds = (ids: string[]) => {
      if (ids.length === 0) return;
      setEphemeralPreviews((prev) => {
        const next = new Map(prev);
        for (const id of ids) {
          next.delete(id);
          const timer = timers.get(id);
          if (timer !== undefined) {
            window.clearTimeout(timer);
            timers.delete(id);
          }
        }
        return next;
      });
    };

    const removeActor = (actor: string) => {
      setActiveActorsBySection((prev) => {
        let changed = false;
        const next = new Map<string, string[]>();
        for (const [sec, actors] of prev.entries()) {
          const filtered = actors.filter((a) => a !== actor);
          if (filtered.length !== actors.length) {
            changed = true;
          }
          if (filtered.length > 0) {
            next.set(sec, filtered);
          }
        }
        return changed ? next : prev;
      });
    };

    const unsub = subscribeDocumentEvents(docId, currentRevRef.current, {
      onRevision(delta) {
        if (delta.cleared_preview_ids && delta.cleared_preview_ids.length > 0) {
          if (currentRevRef.current >= delta.revision) {
            clearPreviewIds(delta.cleared_preview_ids);
          } else {
            const prevList =
              pendingClearedByRevRef.current.get(delta.revision) ?? [];
            pendingClearedByRevRef.current.set(delta.revision, [
              ...prevList,
              ...delta.cleared_preview_ids,
            ]);
          }
        }
        void queryClient.invalidateQueries({
          queryKey: ["doc", docId],
        });
      },
      onEphemeralPreview(preview) {
        setEphemeralPreviews((prev) => {
          const existing = prev.get(preview.preview_id);
          if (
            existing?.seq !== undefined &&
            existing.seq !== null &&
            preview.seq !== undefined &&
            preview.seq !== null &&
            preview.seq < existing.seq
          ) {
            return prev;
          }
          const next = new Map(prev);
          if (preview.status === "streaming") {
            next.set(preview.preview_id, preview);
          } else {
            next.delete(preview.preview_id);
          }
          return next;
        });

        const oldTimer = timers.get(preview.preview_id);
        if (oldTimer !== undefined) {
          window.clearTimeout(oldTimer);
        }
        const ttl = preview.ttl_ms ?? 15_000;
        const timerId = window.setTimeout(() => {
          clearPreviewIds([preview.preview_id]);
        }, ttl);
        timers.set(preview.preview_id, timerId);
      },
      onEphemeralClear(clear) {
        clearPreviewIds(clear.preview_ids);
      },
      onAgentPresence(presence) {
        const prevTimer = presenceTimers.get(presence.actor);
        if (prevTimer !== undefined) {
          window.clearTimeout(prevTimer);
          presenceTimers.delete(presence.actor);
        }
        if (presence.status === "idle" || !presence.section_id) {
          removeActor(presence.actor);
          return;
        }
        const sectionId = presence.section_id;
        setActiveActorsBySection((prev) => {
          const next = new Map<string, string[]>();
          for (const [sec, actors] of prev.entries()) {
            const filtered = actors.filter((a) => a !== presence.actor);
            if (filtered.length > 0) {
              next.set(sec, filtered);
            }
          }
          const curr = next.get(sectionId) ?? [];
          next.set(sectionId, [...curr, presence.actor]);
          return next;
        });
        const timerId = window.setTimeout(() => {
          removeActor(presence.actor);
          presenceTimers.delete(presence.actor);
        }, 10_000);
        presenceTimers.set(presence.actor, timerId);
      },
      onCommentUpdated() {
        void queryClient.invalidateQueries({
          queryKey: ["comments", docId],
        });
      },
      onSyncRequired() {
        void queryClient.invalidateQueries({
          queryKey: ["doc", docId],
        });
      },
    });

    return () => {
      unsub();
      for (const timer of timers.values()) {
        window.clearTimeout(timer);
      }
      timers.clear();
      for (const timer of presenceTimers.values()) {
        window.clearTimeout(timer);
      }
      presenceTimers.clear();
    };
  }, [docId, enabled, queryClient]);

  return { ephemeralPreviews, activeActorsBySection };
}

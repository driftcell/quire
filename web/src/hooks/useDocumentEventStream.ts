import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { subscribeDocumentEvents } from "@/api/sse";
import type { EphemeralPreviewEvent } from "@/contracts/report-protocol";

export interface UseDocumentEventStreamOptions {
  docId: string | null;
  enabled: boolean;
  currentRev: number;
  isRunActive?: boolean;
}

export function useDocumentEventStream({
  docId,
  enabled,
  currentRev,
  isRunActive = true,
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
  const currentRevRef = useRef<number>(currentRev);
  const highestSeenRevRef = useRef<number>(currentRev);

  useEffect(() => {
    currentRevRef.current = currentRev;
    if (currentRev > highestSeenRevRef.current) {
      highestSeenRevRef.current = currentRev;
    }
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

  // When a session turn finishes, fails, or is interrupted (`isRunActive` becomes false),
  // sweep any orphaned ephemeral previews after a short settle window so ghost cards
  // never remain stuck on the canvas even if a network clear frame was dropped.
  useEffect(() => {
    if (isRunActive) return undefined;
    const sweepTimer = window.setTimeout(() => {
      setEphemeralPreviews((prev) => {
        if (prev.size === 0) return prev;
        for (const timer of previewTimersRef.current.values()) {
          window.clearTimeout(timer);
        }
        previewTimersRef.current.clear();
        return new Map();
      });
      setActiveActorsBySection((prev) => (prev.size === 0 ? prev : new Map()));
    }, 1200);
    return () => {
      window.clearTimeout(sweepTimer);
    };
  }, [isRunActive]);

  useEffect(() => {
    if (!docId || !enabled) return undefined;
    const timers = previewTimersRef.current;
    const presenceTimers = presenceTimersRef.current;

    const clearPreviewIds = (ids: string[]) => {
      if (ids.length === 0) return;
      setEphemeralPreviews((prev) => {
        let changed = false;
        const next = new Map(prev);
        for (const id of ids) {
          if (next.delete(id)) {
            changed = true;
          }
          const timer = timers.get(id);
          if (timer !== undefined) {
            window.clearTimeout(timer);
            timers.delete(id);
          }
        }
        return changed ? next : prev;
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

    const unsub = subscribeDocumentEvents(
      docId,
      () => Math.max(currentRevRef.current, highestSeenRevRef.current),
      {
        onRevision(delta) {
          if (delta.revision > highestSeenRevRef.current) {
            highestSeenRevRef.current = delta.revision;
          }
          if (
            delta.cleared_preview_ids &&
            delta.cleared_preview_ids.length > 0
          ) {
            if (currentRevRef.current >= delta.revision) {
              clearPreviewIds(delta.cleared_preview_ids);
            } else {
              const prevList =
                pendingClearedByRevRef.current.get(delta.revision) ?? [];
              pendingClearedByRevRef.current.set(delta.revision, [
                ...prevList,
                ...delta.cleared_preview_ids,
              ]);
              // Safety fallback: if the doc query refetch takes longer than 1.5s,
              // clear the preview IDs anyway so they don't linger behind committed blocks.
              const fallbackIds = [...delta.cleared_preview_ids];
              window.setTimeout(() => {
                clearPreviewIds(fallbackIds);
              }, 1500);
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
      },
    );

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

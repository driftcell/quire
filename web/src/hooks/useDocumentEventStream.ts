import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { getDocument, getDocumentSync } from "@/api/quire";
import { subscribeDocumentEvents } from "@/api/sse";
import type { EphemeralPreviewEvent } from "@/contracts/report-protocol";
import { applyDocumentSync, type DocumentResponse } from "@/lib/document";
import { LatestRevisionSync } from "@/lib/latestRevisionSync";

export interface UseDocumentEventStreamOptions {
  docId: string | null;
  enabled: boolean;
  currentRev: number;
  /** Historical revisions must not invalidate a document before its snapshot is ready. */
  isDocumentReady?: boolean;
  isRunActive?: boolean;
}

const DOCUMENT_SYNC_BATCH_MS = 32;

export function useDocumentEventStream({
  docId,
  enabled,
  currentRev,
  isDocumentReady = true,
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
  const previousDocIdRef = useRef<string | null>(docId);
  const documentReadyRef = useRef(isDocumentReady);
  const scheduleSyncRef = useRef<
    ((revision: number, requireFull?: boolean) => void) | null
  >(null);

  useEffect(() => {
    documentReadyRef.current = isDocumentReady;
    if (previousDocIdRef.current !== docId) {
      previousDocIdRef.current = docId;
      highestSeenRevRef.current = currentRev;
      pendingClearedByRevRef.current.clear();
      setEphemeralPreviews(new Map());
      setActiveActorsBySection(new Map());
    }
    currentRevRef.current = currentRev;
    if (currentRev > highestSeenRevRef.current) {
      highestSeenRevRef.current = currentRev;
    }
    if (pendingClearedByRevRef.current.size > 0) {
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
    }
    if (isDocumentReady && highestSeenRevRef.current > currentRev) {
      scheduleSyncRef.current?.(highestSeenRevRef.current);
    }
  }, [currentRev, docId, isDocumentReady]);

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
    let cancelled = false;
    let syncTimer: number | null = null;
    let fullSyncRequired = false;
    const queryKey = ["doc", docId];

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

    const sync = new LatestRevisionSync(
      () => currentRevRef.current,
      async (fromRevision) => {
        const cached = queryClient.getQueryData<DocumentResponse>(queryKey);
        if (!cached) {
          throw new Error("Document snapshot is not ready for synchronization");
        }
        let next: DocumentResponse;
        if (fullSyncRequired || cached.current_rev !== fromRevision) {
          fullSyncRequired = false;
          next = await getDocument(docId);
        } else {
          try {
            const delta = await getDocumentSync(docId, fromRevision);
            next = applyDocumentSync(cached, delta);
          } catch {
            // A pruned revision, stale cache or malformed delta must never be
            // applied partially. The canonical full snapshot is the recovery path.
            next = await getDocument(docId);
            fullSyncRequired = false;
          }
        }
        if (cancelled) {
          throw new Error("Document synchronization was cancelled");
        }
        if (next.current_rev < fromRevision) {
          throw new Error(
            "Document synchronization returned an older revision",
          );
        }
        currentRevRef.current = next.current_rev;
        highestSeenRevRef.current = Math.max(
          highestSeenRevRef.current,
          next.current_rev,
        );
        queryClient.setQueryData(queryKey, next);
        return next.current_rev;
      },
    );

    const scheduleSync = (revision: number, requireFull = false) => {
      if (cancelled || !documentReadyRef.current) return;
      highestSeenRevRef.current = Math.max(highestSeenRevRef.current, revision);
      fullSyncRequired ||= requireFull;
      if (syncTimer !== null) return;
      syncTimer = window.setTimeout(() => {
        syncTimer = null;
        void sync
          .request(highestSeenRevRef.current, fullSyncRequired)
          .catch(() => {
            if (!cancelled) {
              // If both incremental and full recovery fail, surface the error
              // through the existing React Query document state.
              void queryClient.invalidateQueries({ queryKey });
            }
          });
      }, DOCUMENT_SYNC_BATCH_MS);
    };
    scheduleSyncRef.current = scheduleSync;

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
              // Safety fallback: if the doc sync takes longer than 1.5s,
              // clear the preview IDs anyway so they don't linger behind committed blocks.
              const fallbackIds = [...delta.cleared_preview_ids];
              window.setTimeout(() => {
                clearPreviewIds(fallbackIds);
              }, 1500);
            }
          }
          if (
            documentReadyRef.current &&
            delta.revision > currentRevRef.current
          ) {
            scheduleSync(delta.revision);
          }
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
          const rawTtl = preview.ttl_ms ?? 15_000;
          const ttl = Number.isFinite(rawTtl)
            ? Math.min(Math.max(Math.trunc(rawTtl), 100), 300_000)
            : 15_000;
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
          if (documentReadyRef.current) {
            scheduleSync(highestSeenRevRef.current, true);
          }
        },
      },
    );

    if (
      documentReadyRef.current &&
      highestSeenRevRef.current > currentRevRef.current
    ) {
      scheduleSync(highestSeenRevRef.current);
    }

    return () => {
      cancelled = true;
      if (scheduleSyncRef.current === scheduleSync) {
        scheduleSyncRef.current = null;
      }
      if (syncTimer !== null) {
        window.clearTimeout(syncTimer);
      }
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

import { useCallback, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { deleteSessionQueueItem, updateSessionQueueItem } from "@/api/runtime";
import type {
  PendingQueueItem,
  SessionDetail,
} from "@/contracts/session-protocol";
import { errorMessage } from "@/lib/errors";
import { parseCommentRefs, type CommentRefPayload } from "@/lib/commentRefXml";

export interface UseSessionQueueActionsOptions {
  sessionId: string;
  effectiveDocId: string | null;
  pendingQueue: PendingQueueItem[];
  showChat: () => void;
}

export function useSessionQueueActions({
  sessionId,
  effectiveDocId,
  pendingQueue,
  showChat,
}: UseSessionQueueActionsOptions) {
  const queryClient = useQueryClient();
  const [pendingCommentRefs, setPendingCommentRefs] = useState<
    CommentRefPayload[]
  >([]);
  const [composerPrefill, setComposerPrefill] = useState<{
    key: string;
    text: string;
  } | null>(null);
  const [busyQueueIds, setBusyQueueIds] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const busyQueueIdsRef = useRef<Set<string>>(new Set());

  const runQueueAction = useCallback(
    async (queueId: string, task: () => Promise<void>) => {
      if (busyQueueIdsRef.current.has(queueId)) {
        return;
      }
      busyQueueIdsRef.current.add(queueId);
      setBusyQueueIds(new Set(busyQueueIdsRef.current));
      try {
        await task();
      } catch (error) {
        toast.error(errorMessage(error, "队列操作失败，请稍后重试"));
      } finally {
        busyQueueIdsRef.current.delete(queueId);
        setBusyQueueIds(new Set(busyQueueIdsRef.current));
      }
    },
    [],
  );

  const removePendingQueueItemLocal = useCallback(
    (queueId: string) => {
      queryClient.setQueryData<SessionDetail>(
        ["session", sessionId],
        (current) =>
          current
            ? {
                ...current,
                pending_queue: (current.pending_queue ?? []).filter(
                  (item) => item.queue_id !== queueId,
                ),
              }
            : current,
      );
    },
    [queryClient, sessionId],
  );

  const handleUpdateQueueItem = useCallback(
    async (queueId: string, content: string) => {
      await runQueueAction(queueId, async () => {
        const updated = await updateSessionQueueItem(
          sessionId,
          queueId,
          content,
        );
        queryClient.setQueryData<SessionDetail>(
          ["session", sessionId],
          (current) =>
            current
              ? {
                  ...current,
                  pending_queue: (current.pending_queue ?? []).map((item) =>
                    item.queue_id === queueId ? updated : item,
                  ),
                }
              : current,
        );
      });
    },
    [queryClient, runQueueAction, sessionId],
  );

  const handleRecallQueueItem = useCallback(
    async (item: PendingQueueItem) => {
      await runQueueAction(item.queue_id, async () => {
        await deleteSessionQueueItem(sessionId, item.queue_id);
        removePendingQueueItemLocal(item.queue_id);
        const parsed = parseCommentRefs(item.content);
        if (item.source === "comment") {
          setPendingCommentRefs([]);
          setComposerPrefill({
            key: item.queue_id,
            text:
              parsed.refs
                .map((ref) => ref.comment)
                .filter(Boolean)
                .join("\n\n") || parsed.body,
          });
        } else {
          setPendingCommentRefs(parsed.refs);
          setComposerPrefill({
            key: item.queue_id,
            text: parsed.refs.length > 0 ? parsed.body : item.content,
          });
        }
        showChat();
        if (item.source === "comment" && effectiveDocId) {
          void queryClient.invalidateQueries({
            queryKey: ["comments", effectiveDocId],
          });
        }
      });
    },
    [
      effectiveDocId,
      queryClient,
      removePendingQueueItemLocal,
      runQueueAction,
      sessionId,
      showChat,
    ],
  );

  const handleDeletePending = useCallback(
    async (queueId: string) => {
      const item = pendingQueue.find(
        (candidate) => candidate.queue_id === queueId,
      );
      await runQueueAction(queueId, async () => {
        await deleteSessionQueueItem(sessionId, queueId);
        removePendingQueueItemLocal(queueId);
        if (item?.source === "comment" && effectiveDocId) {
          void queryClient.invalidateQueries({
            queryKey: ["comments", effectiveDocId],
          });
        }
      });
    },
    [
      effectiveDocId,
      pendingQueue,
      queryClient,
      removePendingQueueItemLocal,
      runQueueAction,
      sessionId,
    ],
  );

  return {
    pendingCommentRefs,
    setPendingCommentRefs,
    composerPrefill,
    busyQueueIds,
    handleUpdateQueueItem,
    handleRecallQueueItem,
    handleDeletePending,
  };
}

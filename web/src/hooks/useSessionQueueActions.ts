import { useCallback, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { deleteSessionQueueItem, updateSessionQueueItem } from "@/api/runtime";
import type {
  PendingQueueItem,
  SessionDetail,
} from "@/contracts/session-protocol";
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
  const [queueBusyId, setQueueBusyId] = useState<string | null>(null);

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
      if (queueBusyId) return;
      setQueueBusyId(queueId);
      try {
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
      } finally {
        setQueueBusyId(null);
      }
    },
    [queryClient, queueBusyId, sessionId],
  );

  const handleRecallQueueItem = useCallback(
    async (item: PendingQueueItem) => {
      if (queueBusyId) return;
      setQueueBusyId(item.queue_id);
      try {
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
      } finally {
        setQueueBusyId(null);
      }
    },
    [
      effectiveDocId,
      queryClient,
      queueBusyId,
      removePendingQueueItemLocal,
      sessionId,
      showChat,
    ],
  );

  const handleDeletePending = useCallback(
    async (queueId: string) => {
      if (queueBusyId) return;
      const item = pendingQueue.find(
        (candidate) => candidate.queue_id === queueId,
      );
      setQueueBusyId(queueId);
      try {
        await deleteSessionQueueItem(sessionId, queueId);
        removePendingQueueItemLocal(queueId);
        if (item?.source === "comment" && effectiveDocId) {
          void queryClient.invalidateQueries({
            queryKey: ["comments", effectiveDocId],
          });
        }
      } finally {
        setQueueBusyId(null);
      }
    },
    [
      effectiveDocId,
      pendingQueue,
      queryClient,
      queueBusyId,
      removePendingQueueItemLocal,
      sessionId,
    ],
  );

  return {
    pendingCommentRefs,
    setPendingCommentRefs,
    composerPrefill,
    handleUpdateQueueItem,
    handleRecallQueueItem,
    handleDeletePending,
  };
}

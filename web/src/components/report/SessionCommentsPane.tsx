import { useQueryClient } from "@tanstack/react-query";
import { CommentsPanel } from "@/components/report/CommentsDrawer";
import {
  deleteSessionComment,
  replySessionComment,
  updateSessionCommentStatus,
} from "@/api/runtime";
import type { CommentThread, CommentStatus } from "@/contracts/report-protocol";

export interface SessionCommentsPaneProps {
  sessionId: string;
  docId: string;
  threads: CommentThread[];
  activeThreadId: string | null;
  pendingThreadIds: ReadonlySet<string>;
  isMobile: boolean;
  onClose: () => void;
  onSelectThread: (thread: CommentThread) => void;
  onSelectBlock: (blockId: string) => void;
  onArchived: (threadId: string) => void;
  onDeleted: (thread: CommentThread) => void;
}

export function SessionCommentsPane({
  sessionId,
  docId,
  threads,
  activeThreadId,
  pendingThreadIds,
  isMobile,
  onClose,
  onSelectThread,
  onSelectBlock,
  onArchived,
  onDeleted,
}: SessionCommentsPaneProps) {
  const queryClient = useQueryClient();

  const handleReply = async (threadId: string, body: string) => {
    await replySessionComment(sessionId, threadId, { body });
    void queryClient.invalidateQueries({
      queryKey: ["comments", docId],
    });
  };

  const handleUpdateStatus = async (
    threadId: string,
    status: CommentStatus,
  ) => {
    await updateSessionCommentStatus(sessionId, threadId, status);
    void queryClient.invalidateQueries({
      queryKey: ["comments", docId],
    });
    if (status === "archived") {
      onArchived(threadId);
    }
  };

  const handleDeleteThread = async (thread: CommentThread) => {
    await deleteSessionComment(sessionId, thread.thread_id);
    onDeleted(thread);
    void queryClient.invalidateQueries({
      queryKey: ["comments", docId],
    });
    void queryClient.invalidateQueries({
      queryKey: ["session", sessionId],
    });
  };

  return (
    <CommentsPanel
      docId={docId}
      threads={threads}
      activeThreadId={activeThreadId}
      pendingThreadIds={pendingThreadIds}
      onClose={isMobile ? undefined : onClose}
      onSelectThread={onSelectThread}
      onSelectBlock={onSelectBlock}
      onReply={handleReply}
      onUpdateStatus={handleUpdateStatus}
      onDeleteThread={handleDeleteThread}
    />
  );
}

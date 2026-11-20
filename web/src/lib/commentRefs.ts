import type { SessionCommentRef } from "@/contracts/session-protocol";

export interface PendingMessageDraft {
  content: string;
  comment_refs: SessionCommentRef[];
}

/** Text shown while editing a queued message with structured comment references. */
export function pendingMessageDraftForEdit(
  content: string,
  commentRefs: SessionCommentRef[] = [],
): string {
  if (commentRefs.length > 0 && content.trim().length === 0) {
    return commentRefs.map((ref) => ref.comment).join("\n\n");
  }
  return content;
}

/** Apply an inline edit without changing the queued message's structured shape. */
export function updatePendingMessageDraft(
  content: string,
  commentRefs: SessionCommentRef[],
  editedText: string,
): PendingMessageDraft {
  const text = editedText.trim();
  if (commentRefs.length === 0) {
    return { content: text, comment_refs: [] };
  }
  if (content.trim().length > 0) {
    return { content: text, comment_refs: commentRefs };
  }

  const parts = text
    .split(/\n{2,}/)
    .map((part) => part.trim())
    .filter(Boolean);
  if (parts.length <= 1) {
    return {
      content: "",
      comment_refs: commentRefs.map((ref) => ({ ...ref, comment: text })),
    };
  }
  return {
    content: "",
    comment_refs: commentRefs.map((ref, index) => ({
      ...ref,
      comment: parts[index] ?? ref.comment,
    })),
  };
}

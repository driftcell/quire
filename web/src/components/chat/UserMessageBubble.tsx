import { MessageAttachments } from "@/components/chat/MessageAttachments";
import { UserCommentRefCards } from "@/components/chat/PendingUserBubble";
import { Message } from "@/components/ui/message";
import type {
  AttachmentMeta,
  SessionCommentRef,
} from "@/contracts/session-protocol";

export interface UserMessageBubbleProps {
  body: string;
  commentRefs?: SessionCommentRef[];
  attachmentIds?: string[];
  attachmentMap: Map<string, AttachmentMeta>;
  onJumpToSection?: (sectionId: string) => void;
}

/** Shared right-aligned user surface for transcript and inline trace messages. */
export function UserMessageBubble({
  body,
  commentRefs = [],
  attachmentIds = [],
  attachmentMap,
  onJumpToSection,
}: UserMessageBubbleProps) {
  return (
    <Message align="end" className="items-end">
      <div className="flex max-w-[85%] flex-col items-end gap-1">
        {commentRefs.length > 0 && (
          <div className="w-full">
            <UserCommentRefCards
              refs={commentRefs}
              onJumpToSection={onJumpToSection}
            />
          </div>
        )}
        <MessageAttachments ids={attachmentIds} attachmentMap={attachmentMap} />
        {body.length > 0 && (
          <div className="w-fit max-w-full rounded-2xl border border-border/80 bg-secondary/90 px-4 py-3 text-sm leading-6.5 break-words whitespace-pre-wrap text-foreground">
            {body}
          </div>
        )}
      </div>
    </Message>
  );
}

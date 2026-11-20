import { useEffect, useRef, useState } from "react";
import {
  ArrowUpIcon,
  CornerUpLeftIcon,
  PencilIcon,
  Trash2Icon,
} from "lucide-react";
import { MessageAttachments } from "@/components/chat/MessageAttachments";
import { Message, MessageGroup } from "@/components/ui/message";
import { Textarea } from "@/components/ui/textarea";
import type {
  AttachmentMeta,
  PendingQueueItem,
} from "@/contracts/session-protocol";
import {
  editableCommentRefText,
  parseCommentRefs,
  updateCommentRefContent,
  type CommentRefPayload,
} from "@/lib/commentRefXml";

export function UserCommentRefCards({
  refs,
  onJumpToSection,
}: {
  refs: CommentRefPayload[];
  onJumpToSection?: (sectionId: string) => void;
}) {
  return (
    <div className="mb-1.5 flex w-full flex-col gap-1.5">
      {refs.map((ref) => (
        <button
          key={ref.thread_id}
          type="button"
          onClick={() => onJumpToSection?.(ref.block_id)}
          className="w-full cursor-pointer rounded-md border-l-2 border-border/80 bg-muted/35 px-3 py-2 text-left text-xs transition-colors hover:bg-muted/55 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <div className="line-clamp-1 text-[11px] text-foreground">
            {ref.quote ? `“${ref.quote}”` : ref.comment}
          </div>
          {ref.quote && (
            <div className="mt-0.5 line-clamp-1 text-[10px] text-muted-foreground">
              {ref.comment}
            </div>
          )}
        </button>
      ))}
    </div>
  );
}

export function PendingUserBubble({
  item,
  index,
  count,
  attachmentMap,
  isBusy = false,
  onJumpToSection,
  onEdit,
  onRecall,
  onDelete,
}: {
  item: PendingQueueItem;
  index: number;
  count: number;
  attachmentMap: Map<string, AttachmentMeta>;
  isBusy?: boolean;
  onJumpToSection?: (sectionId: string) => void;
  onEdit?: (queueId: string, content: string) => Promise<void>;
  onRecall?: (item: PendingQueueItem) => Promise<void>;
  onDelete?: (queueId: string) => Promise<void>;
}) {
  const parsed = parseCommentRefs(item.content);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
    }
  }, [editing]);

  const startEditing = () => {
    setDraft(editableCommentRefText(item.content));
    setEditing(true);
  };

  const save = async () => {
    const trimmed = draft.trim();
    if (!trimmed || !onEdit || isBusy) {
      return;
    }
    await onEdit(item.queue_id, updateCommentRefContent(item.content, trimmed));
    setEditing(false);
  };

  const runAction = async (action?: () => Promise<void>) => {
    if (!action || isBusy) return;
    await action();
  };

  return (
    <MessageGroup
      className="gap-1"
      data-queue-id={item.queue_id}
      data-queue-busy={isBusy ? "true" : "false"}
    >
      <Message align="end" className="items-end">
        <div
          className={`flex max-w-[85%] flex-col items-end transition-opacity ${
            isBusy ? "opacity-60" : ""
          }`}
        >
          {parsed.refs.length > 0 && (
            <div className="w-full">
              <UserCommentRefCards
                refs={parsed.refs}
                onJumpToSection={onJumpToSection}
              />
            </div>
          )}
          <MessageAttachments
            ids={item.attachment_ids ?? []}
            attachmentMap={attachmentMap}
          />
          {editing ? (
            <div className="w-full space-y-1.5">
              <Textarea
                ref={inputRef}
                rows={2}
                disabled={isBusy}
                className="min-h-10 text-sm disabled:opacity-60"
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    setEditing(false);
                    event.currentTarget.blur();
                    return;
                  }
                  if (
                    event.key === "Enter" &&
                    !event.shiftKey &&
                    !event.nativeEvent.isComposing
                  ) {
                    event.preventDefault();
                    void save();
                  }
                }}
              />
              <div className="flex justify-end gap-2 text-[11px] text-muted-foreground">
                <button
                  type="button"
                  className="hover:text-foreground"
                  onClick={() => setEditing(false)}
                >
                  取消
                </button>
                <button
                  type="button"
                  className="hover:text-foreground disabled:opacity-50"
                  disabled={isBusy || !draft.trim()}
                  onClick={() => void save()}
                >
                  保存
                </button>
              </div>
            </div>
          ) : (
            (parsed.refs.length === 0 || parsed.body.length > 0) && (
              <div className="w-fit max-w-full rounded-2xl border border-dashed border-border/80 bg-muted/50 px-4 py-2.5 text-sm leading-6 whitespace-pre-wrap text-foreground/85">
                {parsed.refs.length > 0 ? parsed.body : item.content}
              </div>
            )
          )}
          <div className="mt-1 flex items-center justify-end gap-2 px-1 text-[11px] text-muted-foreground">
            <span className="inline-flex items-center gap-1">
              <ArrowUpIcon className="size-3" />
              等待中{count > 1 ? ` · #${index + 1}` : ""}
            </span>
            <div className="flex items-center gap-1">
              <button
                type="button"
                aria-label="编辑排队消息"
                title="编辑"
                disabled={isBusy}
                className="rounded p-0.5 hover:bg-muted hover:text-foreground disabled:opacity-50"
                onClick={startEditing}
              >
                <PencilIcon className="size-3" />
              </button>
              <button
                type="button"
                aria-label="撤回到输入框"
                title="撤回到输入框"
                disabled={isBusy}
                className="rounded p-0.5 hover:bg-muted hover:text-foreground disabled:opacity-50"
                onClick={() => {
                  if (onRecall) void runAction(() => onRecall(item));
                }}
              >
                <CornerUpLeftIcon className="size-3" />
              </button>
              <button
                type="button"
                aria-label="删除排队消息"
                title="删除"
                disabled={isBusy}
                className="rounded p-0.5 hover:bg-muted hover:text-destructive disabled:opacity-50"
                onClick={() => {
                  if (onDelete) void runAction(() => onDelete(item.queue_id));
                }}
              >
                <Trash2Icon className="size-3" />
              </button>
            </div>
          </div>
        </div>
      </Message>
    </MessageGroup>
  );
}

import { isNativeImeKeyEvent } from "@/lib/ime";
import { memo, useMemo, useRef, useState } from "react";
import { ArrowUpIcon, ArrowUpRightIcon, CheckIcon, XIcon } from "lucide-react";
import { toast } from "sonner";
import { toPopoverAnchor } from "@/components/report/reportDom";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTitle } from "@/components/ui/popover";
import { Textarea } from "@/components/ui/textarea";
import type {
  CommentThread,
  TextQuoteSelector,
} from "@/contracts/report-protocol";
import { errorMessage } from "@/lib/errors";

export interface FloatingCommentAnchor {
  blockId: string;
  endBlockId?: string;
  blockIds?: string[];
  quote?: TextQuoteSelector;
  endQuote?: TextQuoteSelector;
  anchorSnapshot?: string;
  /** Viewport coordinates of the trigger element or selection rect. */
  rect: {
    top: number;
    left: number;
    bottom: number;
    right: number;
    width: number;
    height: number;
  };
}

export interface FloatingCommentPopoverProps {
  anchor: FloatingCommentAnchor | null;
  existingCount?: number;
  existingThreads?: CommentThread[];
  onClose: () => void;
  onAddComment: (anchor: FloatingCommentAnchor, body: string) => Promise<void>;
  onReplyThread?: (threadId: string, body: string) => Promise<void>;
  onResolveThread?: (threadId: string) => Promise<void>;
  onSelectThread?: (thread: CommentThread) => void;
}

export const FloatingCommentPopover = memo(function FloatingCommentPopover({
  anchor,
  existingCount,
  existingThreads = [],
  onClose,
  onAddComment,
  onReplyThread,
  onResolveThread,
  onSelectThread,
}: FloatingCommentPopoverProps) {
  const [newBody, setNewBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [replyingThreadId, setReplyingThreadId] = useState<string | null>(null);
  const [replyDraft, setReplyDraft] = useState("");
  const [threadBusyId, setThreadBusyId] = useState<string | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const popoverAnchor = useMemo(
    () => (anchor ? toPopoverAnchor(anchor.rect) : null),
    [anchor],
  );

  if (!anchor) {
    return null;
  }

  const openThreads = existingThreads.filter(
    (thread) => thread.status === "open",
  );
  const count = existingCount ?? openThreads.length;

  const submit = () => {
    const trimmed = newBody.trim();
    if (!trimmed || busy) return;
    void (async () => {
      setBusy(true);
      try {
        await onAddComment(anchor, trimmed);
        setNewBody("");
        onClose();
      } catch (error) {
        toast.error(errorMessage(error, "添加批注失败，请稍后重试"));
      } finally {
        setBusy(false);
      }
    })();
  };

  const submitReply = (threadId: string) => {
    const trimmed = replyDraft.trim();
    if (!trimmed || !onReplyThread || threadBusyId) return;
    void (async () => {
      setThreadBusyId(threadId);
      try {
        await onReplyThread(threadId, trimmed);
        setReplyDraft("");
        setReplyingThreadId(null);
      } catch (error) {
        toast.error(errorMessage(error, "回复批注失败，请稍后重试"));
      } finally {
        setThreadBusyId(null);
      }
    })();
  };

  const resolveThread = (threadId: string) => {
    if (!onResolveThread || threadBusyId) return;
    void (async () => {
      setThreadBusyId(threadId);
      try {
        await onResolveThread(threadId);
      } catch (error) {
        toast.error(errorMessage(error, "更新批注状态失败，请稍后重试"));
      } finally {
        setThreadBusyId(null);
      }
    })();
  };

  return (
    <Popover
      open={anchor !== null}
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <PopoverContent
        anchor={popoverAnchor}
        side="right"
        align="start"
        sideOffset={10}
        initialFocus={
          openThreads.length > 0 && !anchor.quote ? false : inputRef
        }
        aria-label="添加批注"
        className="w-[330px] gap-0 rounded-xl border bg-popover p-0 text-popover-foreground shadow-lg print:hidden"
      >
        <div className="flex items-center justify-between border-b px-3.5 py-2.5">
          <PopoverTitle className="text-xs font-medium">
            {openThreads.length > 0 && !anchor.quote
              ? `批注详情 (${openThreads.length})`
              : "添加批注"}
          </PopoverTitle>
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            aria-label="关闭批注浮窗"
            onClick={onClose}
          >
            <XIcon />
          </Button>
        </div>

        <div className="flex flex-col gap-3 p-3.5 text-xs">
          {openThreads.length > 0 ? (
            <div className="no-scrollbar max-h-60 space-y-2.5 overflow-y-auto pr-0.5">
              {openThreads.map((thread) => {
                const isThreadBusy = threadBusyId === thread.thread_id;
                const isReplying = replyingThreadId === thread.thread_id;
                return (
                  <div
                    key={thread.thread_id}
                    data-slot="popover-comment-thread"
                    className="space-y-2 rounded-lg border bg-muted/25 p-2.5"
                  >
                    <div className="flex items-start justify-between gap-2">
                      {thread.quote ? (
                        <p className="line-clamp-2 border-l-2 border-amber-500/60 pl-2 text-[11px] text-muted-foreground">
                          “{thread.quote.exact}”
                        </p>
                      ) : thread.anchor_snapshot ? (
                        <p className="line-clamp-1 border-l-2 border-amber-500/60 pl-2 text-[11px] text-muted-foreground">
                          {thread.anchor_snapshot}
                        </p>
                      ) : (
                        <span className="text-[11px] text-muted-foreground">
                          #{thread.block_id}
                        </span>
                      )}
                      {onSelectThread && (
                        <button
                          type="button"
                          title="在右侧批注栏查看"
                          onClick={() => {
                            onClose();
                            onSelectThread(thread);
                          }}
                          className="inline-flex shrink-0 items-center gap-0.5 rounded px-1 py-0.5 text-[10px] text-muted-foreground hover:bg-muted hover:text-foreground"
                        >
                          <span>批注栏</span>
                          <ArrowUpRightIcon className="size-3" />
                        </button>
                      )}
                    </div>

                    <div className="space-y-1.5">
                      {thread.messages.map((message, index) => (
                        <div
                          key={message.message_id}
                          className={
                            index > 0 ? "border-t border-border/50 pt-1.5" : ""
                          }
                        >
                          {message.author_type === "agent" && (
                            <div className="mb-0.5 text-[9px] font-medium tracking-wide text-primary/80 uppercase">
                              AI 回复
                            </div>
                          )}
                          <p className="whitespace-pre-wrap leading-5 text-foreground">
                            {message.body}
                          </p>
                        </div>
                      ))}
                    </div>

                    {isReplying && onReplyThread && (
                      <div className="flex items-end gap-1.5 rounded-lg border bg-background px-2 py-1.5">
                        <Textarea
                          rows={1}
                          disabled={isThreadBusy}
                          className="min-h-6 resize-none border-0 bg-transparent px-0 py-0 text-xs shadow-none focus-visible:ring-0"
                          placeholder="回复此批注..."
                          value={replyDraft}
                          onChange={(event) =>
                            setReplyDraft(event.target.value)
                          }
                          onKeyDown={(event) => {
                            if (
                              event.key === "Enter" &&
                              !event.shiftKey &&
                              !isNativeImeKeyEvent(event)
                            ) {
                              event.preventDefault();
                              submitReply(thread.thread_id);
                            }
                          }}
                        />
                        <Button
                          type="button"
                          size="icon-xs"
                          variant="ghost"
                          aria-label="发送批注回复"
                          disabled={isThreadBusy || !replyDraft.trim()}
                          onClick={() => submitReply(thread.thread_id)}
                        >
                          <ArrowUpIcon className="size-3.5" />
                        </Button>
                      </div>
                    )}

                    <div className="flex items-center gap-3 border-t border-border/50 pt-1.5 text-[11px] text-muted-foreground">
                      {onReplyThread && (
                        <button
                          type="button"
                          disabled={isThreadBusy}
                          onClick={() => {
                            setReplyingThreadId((prev) =>
                              prev === thread.thread_id
                                ? null
                                : thread.thread_id,
                            );
                            setReplyDraft("");
                          }}
                          className="hover:text-foreground disabled:opacity-50"
                        >
                          回复
                        </button>
                      )}
                      {onResolveThread && (
                        <button
                          type="button"
                          disabled={isThreadBusy}
                          onClick={() => resolveThread(thread.thread_id)}
                          className="inline-flex items-center gap-0.5 hover:text-emerald-600 disabled:opacity-50"
                        >
                          <CheckIcon className="size-3" />
                          <span>标记已解决</span>
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            count > 0 && (
              <p className="text-[11px] text-muted-foreground">
                此块已有 {count} 条批注
              </p>
            )
          )}

          {anchor.quote && (
            <p className="line-clamp-2 border-l-2 border-border pl-2 text-[11px] text-muted-foreground">
              {anchor.anchorSnapshot ?? anchor.quote.exact}
            </p>
          )}
          <Textarea
            ref={inputRef}
            rows={openThreads.length > 0 ? 2 : 3}
            value={newBody}
            placeholder="添加批注..."
            className="text-xs"
            onChange={(event) => setNewBody(event.target.value)}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !isNativeImeKeyEvent(event)
              ) {
                event.preventDefault();
                submit();
              }
            }}
          />
          <div className="flex justify-end">
            <Button
              type="button"
              size="xs"
              disabled={busy || !newBody.trim()}
              onClick={submit}
            >
              添加
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
});

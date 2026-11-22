import { memo, useMemo } from "react";
import { ArrowUpRightIcon, MessageSquarePlusIcon } from "lucide-react";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import type { HoveredCommentAnchor } from "@/components/report/useReportHighlights";
import { toPopoverAnchor } from "@/components/report/reportDom";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent } from "@/components/ui/popover";
import type { CommentThread } from "@/contracts/report-protocol";

export interface CommentHoverCardProps {
  hovered: HoveredCommentAnchor | null;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onDismiss: () => void;
  onOpenPopoverForBlock?: (anchor: FloatingCommentAnchor) => void;
  onSelectThread?: (thread: CommentThread) => void;
}

export const CommentHoverCard = memo(function CommentHoverCard({
  hovered,
  onMouseEnter,
  onMouseLeave,
  onDismiss,
  onOpenPopoverForBlock,
  onSelectThread,
}: CommentHoverCardProps) {
  const popoverAnchor = useMemo(
    () => (hovered ? toPopoverAnchor(hovered.rect) : null),
    [hovered],
  );

  if (!hovered || hovered.threads.length === 0) {
    return null;
  }

  return (
    <Popover open>
      <PopoverContent
        data-slot="comment-hover-card"
        anchor={popoverAnchor}
        side="bottom"
        align="start"
        sideOffset={8}
        initialFocus={false}
        onMouseEnter={onMouseEnter}
        onMouseLeave={onMouseLeave}
        className="w-[310px] gap-0 rounded-xl border bg-popover p-0 text-popover-foreground shadow-lg print:hidden"
      >
        <div className="flex items-center justify-between border-b px-3 py-2">
          <div className="flex items-center gap-1.5 text-xs font-medium">
            <span
              className="size-1.5 shrink-0 rounded-full bg-amber-500"
              aria-hidden="true"
            />
            <span>待处理批注 ({hovered.threads.length})</span>
          </div>
          {onOpenPopoverForBlock && (
            <Button
              type="button"
              size="xs"
              variant="ghost"
              className="h-6 gap-1 px-2 text-[11px] text-muted-foreground hover:text-foreground"
              onClick={() => {
                const target = hovered;
                onDismiss();
                onOpenPopoverForBlock({
                  blockId: target.blockId,
                  rect: target.rect,
                });
              }}
            >
              <MessageSquarePlusIcon className="size-3" />
              <span>回复 / 处理</span>
            </Button>
          )}
        </div>

        <div className="no-scrollbar max-h-64 divide-y divide-border/60 overflow-y-auto p-3 text-xs">
          {hovered.threads.map((thread) => (
            <div
              key={thread.thread_id}
              className="space-y-2 py-2 first:pt-0 last:pb-0"
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
                    title="在右侧批注栏打开"
                    onClick={() => {
                      onDismiss();
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
            </div>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
});

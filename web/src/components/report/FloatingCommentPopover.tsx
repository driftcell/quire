import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  ArchiveIcon,
  GitCompareArrowsIcon,
  MessageSquarePlusIcon,
  PanelRightOpenIcon,
  XIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import type {
  CommentStatus,
  CommentThread,
  TextQuoteSelector,
} from "@/contracts/report-protocol";

export interface FloatingCommentAnchor {
  blockId: string;
  quote?: TextQuoteSelector;
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
  threads: CommentThread[];
  onClose: () => void;
  onAddComment: (
    blockId: string,
    body: string,
    quote?: TextQuoteSelector,
  ) => Promise<void>;
  onReply: (threadId: string, body: string) => Promise<void>;
  onUpdateStatus: (threadId: string, status: CommentStatus) => Promise<void>;
  onOpenDrawer: () => void;
  onViewDiff?: (fromRev: number, toRev: number, blockId: string) => void;
}

const POPOVER_WIDTH = 340;
const VIEWPORT_MARGIN = 12;

export const FloatingCommentPopover = memo(function FloatingCommentPopover({
  anchor,
  threads,
  onClose,
  onAddComment,
  onReply,
  onUpdateStatus,
  onOpenDrawer,
  onViewDiff,
}: FloatingCommentPopoverProps) {
  const cardRef = useRef<HTMLDialogElement>(null);
  const [newBody, setNewBody] = useState("");
  const [replyByThread, setReplyByThread] = useState<Record<string, string>>(
    {},
  );
  const [busy, setBusy] = useState(false);

  const blockThreads = useMemo(() => {
    if (!anchor) return [];
    return threads.filter(
      (t) => t.block_id === anchor.blockId && t.status !== "archived",
    );
  }, [anchor, threads]);

  useEffect(() => {
    if (!anchor) return undefined;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target;
      if (
        target instanceof Node &&
        cardRef.current &&
        !cardRef.current.contains(target)
      ) {
        onClose();
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [anchor, onClose]);

  if (!anchor) {
    return null;
  }

  const vw = typeof window !== "undefined" ? window.innerWidth : 1280;
  const vh = typeof window !== "undefined" ? window.innerHeight : 800;

  let left = anchor.rect.right + 10;
  if (left + POPOVER_WIDTH + VIEWPORT_MARGIN > vw) {
    left = Math.max(
      VIEWPORT_MARGIN,
      Math.min(anchor.rect.left, vw - POPOVER_WIDTH - VIEWPORT_MARGIN),
    );
  }

  const maxCardHeight = 420;
  let top = anchor.rect.bottom + 8;
  if (top + maxCardHeight + VIEWPORT_MARGIN > vh) {
    top = Math.max(VIEWPORT_MARGIN, vh - maxCardHeight - VIEWPORT_MARGIN);
  }

  const submitNew = () => {
    const trimmed = newBody.trim();
    if (!trimmed || busy) return;
    void (async () => {
      setBusy(true);
      try {
        await onAddComment(anchor.blockId, trimmed, anchor.quote);
        setNewBody("");
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <dialog
      ref={cardRef}
      open
      aria-label="块级批注浮窗"
      style={{
        top: `${top}px`,
        left: `${left}px`,
        width: `${POPOVER_WIDTH}px`,
      }}
      className="bg-popover text-popover-foreground fixed z-50 m-0 flex max-h-[420px] flex-col rounded-xl border p-0 shadow-xl ring-1 ring-black/5 dark:ring-white/10 print:hidden"
    >
      <div className="flex items-center justify-between border-b px-3 py-2">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="text-xs font-semibold">批注</span>
          <code className="truncate rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
            #{anchor.blockId}
          </code>
        </div>
        <div className="flex items-center gap-0.5">
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            title="在侧栏查看全部批注"
            aria-label="在侧栏查看全部批注"
            onClick={() => {
              onClose();
              onOpenDrawer();
            }}
          >
            <PanelRightOpenIcon className="size-3.5" />
          </Button>
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            aria-label="关闭批注浮窗"
            onClick={onClose}
          >
            <XIcon className="size-3.5" />
          </Button>
        </div>
      </div>

      <div className="flex-1 space-y-2.5 overflow-y-auto overscroll-contain p-3 text-xs">
        {blockThreads.map((thread) => (
          <div
            key={thread.thread_id}
            className="space-y-2 rounded-lg border bg-card p-2.5"
          >
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge
                variant={thread.status === "addressed" ? "default" : "outline"}
                className="h-4 px-1.5 text-[10px]"
              >
                {thread.status === "addressed"
                  ? `已处理 (rev ${thread.addressed_rev ?? "?"})`
                  : "待处理"}
              </Badge>
              {thread.addressed_rev !== undefined &&
                thread.addressed_rev !== null &&
                onViewDiff && (
                  <button
                    type="button"
                    onClick={() =>
                      onViewDiff(
                        thread.created_rev,
                        thread.addressed_rev ?? thread.created_rev,
                        thread.block_id,
                      )
                    }
                    className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-[10px] text-primary hover:bg-muted"
                  >
                    <GitCompareArrowsIcon className="size-3" />
                    <span>
                      {thread.created_rev}→{thread.addressed_rev}
                    </span>
                  </button>
                )}
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  void onUpdateStatus(thread.thread_id, "archived");
                }}
                className="ml-auto inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted-foreground hover:bg-muted hover:text-foreground"
              >
                <ArchiveIcon className="size-3" />
                <span>归档</span>
              </button>
            </div>

            {thread.quote && (
              <div className="rounded border-l-2 border-amber-500 bg-amber-500/10 px-2 py-1 text-[11px]">
                “{thread.quote.exact}”
              </div>
            )}

            <div className="space-y-1.5">
              {thread.messages.map((msg) => (
                <div
                  key={msg.message_id}
                  className="rounded bg-muted/40 px-2 py-1.5"
                >
                  <div className="mb-0.5 text-[10px] font-medium text-muted-foreground">
                    {msg.author} (
                    {msg.author_type === "agent" ? "Agent" : "人类"})
                  </div>
                  <p className="m-0 whitespace-pre-wrap text-foreground">
                    {msg.body}
                  </p>
                </div>
              ))}
            </div>

            <div className="flex gap-1.5">
              <Textarea
                rows={1}
                className="min-h-7 text-xs"
                placeholder="回复..."
                value={replyByThread[thread.thread_id] ?? ""}
                onChange={(event) => {
                  const val = event.target.value;
                  setReplyByThread((prev) => ({
                    ...prev,
                    [thread.thread_id]: val,
                  }));
                }}
              />
              <Button
                type="button"
                size="xs"
                variant="outline"
                disabled={
                  busy || !(replyByThread[thread.thread_id] ?? "").trim()
                }
                onClick={() => {
                  const text = (replyByThread[thread.thread_id] ?? "").trim();
                  if (!text) return;
                  void (async () => {
                    setBusy(true);
                    try {
                      await onReply(thread.thread_id, text);
                      setReplyByThread((prev) => ({
                        ...prev,
                        [thread.thread_id]: "",
                      }));
                    } finally {
                      setBusy(false);
                    }
                  })();
                }}
              >
                回复
              </Button>
            </div>
          </div>
        ))}

        <div className="space-y-2 rounded-lg border bg-muted/25 p-2.5">
          {anchor.quote && (
            <div className="rounded border-l-2 border-amber-500 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-800 dark:text-amber-200">
              选中：“{anchor.quote.exact}”
            </div>
          )}
          <Textarea
            autoFocus
            rows={2}
            value={newBody}
            placeholder="输入批注或修改建议... (⌘/Ctrl + Enter 发送)"
            className="text-xs"
            onChange={(event) => setNewBody(event.target.value)}
            onKeyDown={(event) => {
              if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                event.preventDefault();
                submitNew();
              }
            }}
          />
          <div className="flex justify-end">
            <Button
              type="button"
              size="xs"
              disabled={busy || !newBody.trim()}
              onClick={submitNew}
            >
              <MessageSquarePlusIcon className="mr-1.5 size-3.5" />
              发送批注
            </Button>
          </div>
        </div>
      </div>
    </dialog>
  );
});

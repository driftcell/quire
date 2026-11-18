import { useState } from "react";
import {
  ArchiveIcon,
  CheckCircle2Icon,
  MessageSquarePlusIcon,
  RotateCcwIcon,
  Trash2Icon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Textarea } from "@/components/ui/textarea";
import type { CommentStatus, CommentThread } from "@/contracts/report-protocol";

export interface CommentsDrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  threads: CommentThread[];
  selectedBlockId: string | null;
  onAddComment: (blockId: string, body: string) => Promise<void>;
  onReply: (threadId: string, body: string) => Promise<void>;
  onUpdateStatus: (threadId: string, status: CommentStatus) => Promise<void>;
  onSelectBlock?: (blockId: string) => void;
}

export function CommentsDrawer({
  open,
  onOpenChange,
  threads,
  selectedBlockId,
  onAddComment,
  onReply,
  onUpdateStatus,
  onSelectBlock,
}: CommentsDrawerProps) {
  const [newBody, setNewBody] = useState("");
  const [replyByThread, setReplyByThread] = useState<Record<string, string>>(
    {},
  );
  const [filter, setFilter] = useState<"active" | "archived">("active");
  const [busy, setBusy] = useState(false);

  const filtered = threads.filter((t) =>
    filter === "archived"
      ? t.status === "archived"
      : t.status === "open" || t.status === "addressed",
  );

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="flex w-full flex-col sm:max-w-md">
        <SheetHeader>
          <SheetTitle className="flex items-center justify-between pr-6">
            <span>批注与审阅</span>
            <div className="flex gap-1">
              <Button
                type="button"
                size="xs"
                variant={filter === "active" ? "default" : "ghost"}
                onClick={() => setFilter("active")}
              >
                待处理 (
                {
                  threads.filter(
                    (t) => t.status === "open" || t.status === "addressed",
                  ).length
                }
                )
              </Button>
              <Button
                type="button"
                size="xs"
                variant={filter === "archived" ? "default" : "ghost"}
                onClick={() => setFilter("archived")}
              >
                已归档 ({threads.filter((t) => t.status === "archived").length})
              </Button>
            </div>
          </SheetTitle>
        </SheetHeader>

        {selectedBlockId && (
          <div className="mt-3 space-y-2 rounded-lg border bg-muted/30 p-3">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>针对块添加批注</span>
              <code className="rounded bg-muted px-1.5 py-0.5 text-[11px]">
                {selectedBlockId}
              </code>
            </div>
            <Textarea
              rows={2}
              value={newBody}
              placeholder="输入批注或修改建议..."
              onChange={(event) => setNewBody(event.target.value)}
            />
            <Button
              type="button"
              size="xs"
              disabled={busy || !newBody.trim()}
              onClick={() => {
                void (async () => {
                  setBusy(true);
                  try {
                    await onAddComment(selectedBlockId, newBody.trim());
                    setNewBody("");
                  } finally {
                    setBusy(false);
                  }
                })();
              }}
            >
              <MessageSquarePlusIcon className="mr-1.5 size-3.5" />
              发送批注
            </Button>
          </div>
        )}

        <div className="mt-4 flex-1 space-y-3 overflow-y-auto pr-1">
          {filtered.length === 0 ? (
            <div className="py-12 text-center text-xs text-muted-foreground">
              {filter === "active" ? "暂无待处理批注" : "暂无已归档批注"}
            </div>
          ) : (
            filtered.map((thread) => (
              <div
                key={thread.thread_id}
                className="space-y-2.5 rounded-xl border bg-card p-3 text-xs"
              >
                <div className="flex flex-wrap items-center gap-1.5">
                  <Badge
                    variant={
                      thread.status === "addressed"
                        ? "default"
                        : thread.status === "archived"
                          ? "secondary"
                          : "outline"
                    }
                  >
                    {thread.status === "open"
                      ? "待处理"
                      : thread.status === "addressed"
                        ? `Agent 已处理 (rev ${thread.addressed_rev ?? "?"})`
                        : "已归档"}
                  </Badge>
                  {thread.anchor_detached ? (
                    <Badge variant="destructive" className="gap-1">
                      <Trash2Icon className="size-3" />
                      原块已删除
                    </Badge>
                  ) : (
                    <button
                      type="button"
                      onClick={() => onSelectBlock?.(thread.block_id)}
                      className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] hover:underline"
                    >
                      #{thread.block_id}
                    </button>
                  )}
                </div>

                {thread.anchor_snapshot && (
                  <blockquote className="border-l-2 border-muted-foreground/30 pl-2 text-[11px] text-muted-foreground italic">
                    {thread.anchor_snapshot}
                  </blockquote>
                )}

                <div className="space-y-2">
                  {thread.messages.map((msg) => (
                    <div
                      key={msg.message_id}
                      className="rounded-lg bg-muted/40 p-2"
                    >
                      <div className="mb-0.5 flex items-center justify-between text-[11px] text-muted-foreground">
                        <span className="font-medium text-foreground">
                          {msg.author} (
                          {msg.author_type === "agent" ? "Agent" : "人类"})
                        </span>
                      </div>
                      <p className="whitespace-pre-wrap text-foreground">
                        {msg.body}
                      </p>
                    </div>
                  ))}
                </div>

                {thread.status !== "archived" && (
                  <div className="space-y-1.5 pt-1">
                    <div className="flex gap-1.5">
                      <Textarea
                        rows={1}
                        className="min-h-8 text-xs"
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
                          busy ||
                          !(replyByThread[thread.thread_id] ?? "").trim()
                        }
                        onClick={() => {
                          const text = (
                            replyByThread[thread.thread_id] ?? ""
                          ).trim();
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
                    <div className="flex justify-end">
                      <Button
                        type="button"
                        size="xs"
                        variant="secondary"
                        disabled={busy}
                        onClick={() => {
                          void onUpdateStatus(thread.thread_id, "archived");
                        }}
                      >
                        <ArchiveIcon className="mr-1 size-3" />
                        归档完成
                      </Button>
                    </div>
                  </div>
                )}

                {thread.status === "archived" && (
                  <div className="flex justify-end">
                    <Button
                      type="button"
                      size="xs"
                      variant="ghost"
                      disabled={busy}
                      onClick={() => {
                        void onUpdateStatus(thread.thread_id, "open");
                      }}
                    >
                      <RotateCcwIcon className="mr-1 size-3" />
                      重新打开
                    </Button>
                  </div>
                )}
              </div>
            ))
          )}
        </div>
        {threads.some((t) => t.status === "addressed") &&
          filter === "active" && (
            <div className="mt-2 flex items-center gap-1.5 rounded bg-emerald-500/10 px-2.5 py-1.5 text-[11px] text-emerald-700 dark:text-emerald-300">
              <CheckCircle2Icon className="size-3.5 shrink-0" />
              <span>Agent 已处理的批注可核对后手动归档。</span>
            </div>
          )}
      </SheetContent>
    </Sheet>
  );
}

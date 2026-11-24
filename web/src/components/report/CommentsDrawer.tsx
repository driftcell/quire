import { isNativeImeKeyEvent } from "@/lib/ime";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  ArrowUpIcon,
  ArrowUpRightIcon,
  GitCompareArrowsIcon,
  MessageSquareIcon,
  XIcon,
} from "lucide-react";
import { getDocumentDiff } from "@/api/quire";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import type {
  Block,
  CommentStatus,
  CommentThread,
} from "@/contracts/report-protocol";
import { toast } from "sonner";
import { errorMessage } from "@/lib/errors";

type CommentViewFilter = "open" | "resolved";

export interface CommentsPanelProps {
  docId: string;
  threads: CommentThread[];
  activeThreadId?: string | null;
  pendingThreadIds?: ReadonlySet<string>;
  readOnly?: boolean;
  onClose?: () => void;
  onSelectThread?: (thread: CommentThread) => void;
  onSelectBlock?: (blockId: string) => void;
  onReply?: (threadId: string, body: string) => Promise<void>;
  onUpdateStatus?: (threadId: string, status: CommentStatus) => Promise<void>;
  onDeleteThread?: (thread: CommentThread) => Promise<void>;
}

function summarizeBlock(block: Block): string {
  if (
    block.type === "heading" ||
    block.type === "paragraph" ||
    block.type === "quote"
  ) {
    return block.text;
  }
  const rawTitle = block.title;
  const title = typeof rawTitle === "string" ? rawTitle : "";
  if (block.type === "echarts") {
    const rawSummary = block.fallback_summary;
    const summary = typeof rawSummary === "string" ? rawSummary : "";
    return `[图表] ${title}${summary ? ` — ${summary}` : ""}`;
  }
  if (block.type === "table") {
    return `[表格] ${title}`;
  }
  return `[HTML] ${title}`;
}

function isResolvedStatus(status: CommentStatus): boolean {
  return status === "addressed" || status === "archived";
}

function statusLabel(status: CommentStatus): string {
  if (status === "open") return "待处理";
  return "已解决";
}

function StatusDot({ status }: { status: CommentStatus | CommentViewFilter }) {
  const color = status === "open" ? "bg-amber-500" : "bg-emerald-500";
  return (
    <span
      className={`size-1.5 shrink-0 rounded-full ${color}`}
      aria-hidden="true"
    />
  );
}

export function CommentsPanel({
  docId,
  threads,
  activeThreadId,
  pendingThreadIds,
  readOnly = false,
  onClose,
  onSelectThread,
  onSelectBlock,
  onReply,
  onUpdateStatus,
  onDeleteThread,
}: CommentsPanelProps) {
  const [filter, setFilter] = useState<CommentViewFilter>("open");
  const [busyThreadIds, setBusyThreadIds] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const busyThreadsRef = useRef<Set<string>>(new Set());
  const [replyThreadId, setReplyThreadId] = useState<string | null>(null);
  const [replyDraft, setReplyDraft] = useState("");
  const replyInputRef = useRef<HTMLTextAreaElement>(null);
  const [diffRange, setDiffRange] = useState<{
    fromRev: number;
    toRev: number;
    blockId: string;
  } | null>(null);

  const counts = useMemo(
    () => ({
      open: threads.filter((thread) => thread.status === "open").length,
      resolved: threads.filter((thread) => isResolvedStatus(thread.status))
        .length,
    }),
    [threads],
  );
  const filtered = useMemo(() => {
    const base = threads.filter((thread) =>
      filter === "open"
        ? thread.status === "open"
        : isResolvedStatus(thread.status),
    );
    const active = threads.find(
      (thread) => thread.thread_id === activeThreadId,
    );
    if (
      !active ||
      base.some((thread) => thread.thread_id === active.thread_id)
    ) {
      return base;
    }
    return [active, ...base];
  }, [activeThreadId, filter, threads]);

  useEffect(() => {
    if (!activeThreadId || threads.length === 0) {
      return undefined;
    }
    const timer = window.setTimeout(() => {
      window.document
        .getElementById(`comment-thread-${activeThreadId}`)
        ?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }, 40);
    return () => window.clearTimeout(timer);
  }, [activeThreadId, threads.length]);

  useEffect(() => {
    if (replyThreadId) {
      replyInputRef.current?.focus();
    }
  }, [replyThreadId]);

  const diffQuery = useQuery({
    queryKey: ["doc-diff", docId, diffRange?.fromRev, diffRange?.toRev],
    queryFn: () =>
      getDocumentDiff(
        docId,
        diffRange?.fromRev ?? 0,
        diffRange?.toRev ?? undefined,
      ),
    enabled: Boolean(diffRange),
  });

  const runThreadAction = async (
    threadId: string,
    task: () => Promise<void>,
  ) => {
    if (busyThreadsRef.current.has(threadId)) {
      return;
    }
    busyThreadsRef.current.add(threadId);
    setBusyThreadIds(new Set(busyThreadsRef.current));
    try {
      await task();
    } catch (error) {
      toast.error(errorMessage(error, "批注操作失败，请稍后重试"));
    } finally {
      busyThreadsRef.current.delete(threadId);
      setBusyThreadIds(new Set(busyThreadsRef.current));
    }
  };

  const submitReply = async (threadId: string) => {
    const body = replyDraft.trim();
    if (!body || !onReply) return;
    await runThreadAction(threadId, async () => {
      await onReply(threadId, body);
      setReplyDraft("");
      setReplyThreadId(null);
    });
  };

  return (
    <>
      <div className="flex h-full min-h-0 flex-col bg-background">
        <div className="flex shrink-0 items-center gap-1 border-b px-3 py-2">
          <div className="flex items-center gap-1.5 text-xs font-medium">
            <MessageSquareIcon className="size-3.5 text-muted-foreground" />
            <span>批注</span>
            {readOnly && (
              <span className="text-[10px] font-normal text-muted-foreground">
                只读
              </span>
            )}
          </div>
          <div className="ml-auto flex items-center gap-0.5">
            {(
              [
                ["open", "待处理", counts.open],
                ["resolved", "已解决", counts.resolved],
              ] as const
            ).map(([value, label, count]) => (
              <Button
                key={value}
                type="button"
                size="xs"
                variant={filter === value ? "secondary" : "ghost"}
                className="h-7 gap-1 px-2 text-[11px] text-muted-foreground"
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                <StatusDot status={value} />
                {label}
                <span className="tabular-nums opacity-70">{count}</span>
              </Button>
            ))}
            {onClose && (
              <Button
                type="button"
                size="icon-xs"
                variant="ghost"
                aria-label="关闭批注面板"
                onClick={onClose}
              >
                <XIcon className="size-3.5" />
              </Button>
            )}
          </div>
        </div>

        <div className="no-scrollbar min-h-0 flex-1 space-y-2 overflow-y-auto overscroll-contain p-3">
          {filtered.length === 0 ? (
            <div className="mx-1 my-8 rounded-xl border border-dashed border-border/90 bg-secondary/40 px-4 py-7 text-center">
              <MessageSquareIcon
                className="mx-auto mb-2 size-4 text-muted-foreground/60"
                aria-hidden="true"
              />
              <p className="text-xs font-medium text-foreground/85">
                {filter === "open" ? "暂无待处理批注" : "暂无已解决批注"}
              </p>
              <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                {filter === "open"
                  ? "在左侧报告画布中划选任意段落、指标或图表，即可添加审校批注或引用至对话。"
                  : "审阅完成的批注归档后将保存在此处，方便随时回溯版本差异。"}
              </p>
            </div>
          ) : (
            filtered.map((thread) => {
              const active = thread.thread_id === activeThreadId;
              const isThreadBusy = busyThreadIds.has(thread.thread_id);
              const resolved = isResolvedStatus(thread.status);
              return (
                <div
                  key={thread.thread_id}
                  id={`comment-thread-${thread.thread_id}`}
                  data-thread-busy={isThreadBusy ? "true" : "false"}
                  aria-busy={isThreadBusy}
                  // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- the card contains nested action buttons, so a native button would be invalid HTML
                  role="button"
                  tabIndex={0}
                  onClick={() => onSelectThread?.(thread)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      onSelectThread?.(thread);
                    }
                  }}
                  className={`cursor-pointer space-y-2 rounded-lg border p-3 text-xs transition-colors ${
                    active
                      ? "border-foreground/25 bg-muted/40"
                      : "border-border bg-card hover:bg-muted/30"
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <StatusDot status={thread.status} />
                    <span className="text-[11px] text-muted-foreground">
                      {statusLabel(thread.status)}
                    </span>
                    {!resolved && pendingThreadIds?.has(thread.thread_id) && (
                      <span className="text-[10px] text-muted-foreground">
                        排队中
                      </span>
                    )}
                    {thread.anchor_detached && (
                      <span className="text-[10px] text-muted-foreground/70">
                        位置已失效
                      </span>
                    )}
                    <div className="ml-auto flex items-center gap-1">
                      {thread.addressed_rev !== undefined &&
                        thread.addressed_rev !== null && (
                          <button
                            type="button"
                            title="查看修订"
                            onClick={(event) => {
                              event.stopPropagation();
                              setDiffRange({
                                fromRev: thread.created_rev,
                                toRev:
                                  thread.addressed_rev ?? thread.created_rev,
                                blockId: thread.block_id,
                              });
                            }}
                            className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-[10px] text-muted-foreground hover:bg-muted hover:text-foreground"
                          >
                            <GitCompareArrowsIcon className="size-3" />r
                            {thread.created_rev}→{thread.addressed_rev}
                          </button>
                        )}
                      {!thread.anchor_detached && (
                        <button
                          type="button"
                          title={`跳转到 #${thread.block_id}`}
                          aria-label="跳转到批注位置"
                          onClick={(event) => {
                            event.stopPropagation();
                            onSelectBlock?.(thread.block_id);
                          }}
                          className="rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
                        >
                          <ArrowUpRightIcon className="size-3.5" />
                        </button>
                      )}
                    </div>
                  </div>

                  {thread.quote ? (
                    <p className="line-clamp-2 border-l-2 border-border pl-2.5 text-[11px] text-muted-foreground">
                      “{thread.quote.exact}”
                    </p>
                  ) : thread.anchor_snapshot ? (
                    <p className="line-clamp-1 text-[11px] text-muted-foreground">
                      {thread.anchor_snapshot}
                    </p>
                  ) : null}

                  <div className="space-y-2">
                    {thread.messages.map((message, index) => (
                      <div
                        key={message.message_id}
                        className={
                          index > 0 ? "border-t border-border/60 pt-2" : ""
                        }
                      >
                        {message.author_type === "agent" && (
                          <div className="mb-0.5 text-[9px] font-medium tracking-wide text-muted-foreground/70 uppercase">
                            AI
                          </div>
                        )}
                        <p className="whitespace-pre-wrap leading-5 text-foreground">
                          {message.body}
                        </p>
                      </div>
                    ))}
                  </div>

                  {!readOnly &&
                    thread.status !== "archived" &&
                    replyThreadId === thread.thread_id && (
                      <div className="flex items-end gap-1.5 rounded-lg border border-input bg-background px-2 py-1.5 transition-colors focus-within:border-ring">
                        <Textarea
                          ref={replyInputRef}
                          rows={1}
                          disabled={isThreadBusy}
                          className="min-h-7 resize-none border-0 bg-transparent px-0 py-0 text-xs shadow-none focus-visible:ring-0 disabled:opacity-60"
                          placeholder="回复…"
                          value={replyDraft}
                          onClick={(event) => event.stopPropagation()}
                          onChange={(event) =>
                            setReplyDraft(event.target.value)
                          }
                          onKeyDown={(event) => {
                            if (event.key === "Escape") {
                              event.preventDefault();
                              setReplyThreadId(null);
                              setReplyDraft("");
                              event.currentTarget.blur();
                              return;
                            }
                            if (
                              event.key === "Enter" &&
                              !event.shiftKey &&
                              !isNativeImeKeyEvent(event)
                            ) {
                              event.preventDefault();
                              void submitReply(thread.thread_id);
                            }
                          }}
                        />
                        <Button
                          type="button"
                          size="icon-xs"
                          variant="ghost"
                          className="shrink-0 text-muted-foreground hover:text-foreground"
                          aria-label="发送回复"
                          disabled={isThreadBusy || !replyDraft.trim()}
                          onClick={(event) => {
                            event.stopPropagation();
                            void submitReply(thread.thread_id);
                          }}
                        >
                          <ArrowUpIcon className="size-3.5" />
                        </Button>
                      </div>
                    )}

                  <div className="flex flex-wrap items-center gap-3 border-t border-border/60 pt-2 text-[11px] text-muted-foreground">
                    {!readOnly && thread.status !== "archived" && onReply && (
                      <button
                        type="button"
                        className="hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
                        disabled={isThreadBusy}
                        onClick={(event) => {
                          event.stopPropagation();
                          setReplyThreadId((current) =>
                            current === thread.thread_id
                              ? null
                              : thread.thread_id,
                          );
                          setReplyDraft("");
                        }}
                      >
                        回复
                      </button>
                    )}
                    {!readOnly &&
                      thread.status === "open" &&
                      onDeleteThread && (
                        <button
                          type="button"
                          className="hover:text-destructive disabled:pointer-events-none disabled:opacity-50"
                          disabled={isThreadBusy}
                          onClick={(event) => {
                            event.stopPropagation();
                            void runThreadAction(thread.thread_id, () =>
                              onDeleteThread(thread),
                            );
                          }}
                        >
                          删除
                        </button>
                      )}
                    {!readOnly &&
                      thread.status === "open" &&
                      onUpdateStatus && (
                        <button
                          type="button"
                          title="标记为已解决并从报告正文隐藏"
                          className="hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
                          disabled={isThreadBusy}
                          onClick={(event) => {
                            event.stopPropagation();
                            void runThreadAction(thread.thread_id, () =>
                              onUpdateStatus(thread.thread_id, "archived"),
                            );
                          }}
                        >
                          归档
                        </button>
                      )}
                    {!readOnly && resolved && onUpdateStatus && (
                      <button
                        type="button"
                        className="hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
                        disabled={isThreadBusy}
                        onClick={(event) => {
                          event.stopPropagation();
                          void runThreadAction(thread.thread_id, () =>
                            onUpdateStatus(thread.thread_id, "open"),
                          );
                        }}
                      >
                        重新打开
                      </button>
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>
      </div>

      <Dialog
        open={Boolean(diffRange)}
        onOpenChange={(isOpen) => {
          if (!isOpen) setDiffRange(null);
        }}
      >
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle className="text-sm">
              修订对比 r{diffRange?.fromRev}→r{diffRange?.toRev}
            </DialogTitle>
          </DialogHeader>
          <div className="no-scrollbar max-h-[60vh] space-y-3 overflow-y-auto text-xs">
            {diffQuery.isLoading && (
              <div className="py-8 text-center text-muted-foreground">
                正在加载版本差异…
              </div>
            )}
            {diffQuery.data && diffQuery.data.blocks.length === 0 && (
              <div className="py-8 text-center text-muted-foreground">
                两个版本间无块级内容变化。
              </div>
            )}
            {diffQuery.data?.blocks.map((entry, idx) => {
              if (entry.type === "added") {
                return (
                  <div
                    key={idx}
                    className="space-y-1 rounded-lg border border-border bg-card p-3"
                  >
                    <div className="text-[11px] text-muted-foreground">
                      + 新增
                    </div>
                    <div className="whitespace-pre-wrap">
                      {summarizeBlock(entry.block)}
                    </div>
                  </div>
                );
              }
              if (entry.type === "removed") {
                return (
                  <div
                    key={idx}
                    className="space-y-1 rounded-lg border border-border bg-muted/40 p-3"
                  >
                    <div className="text-[11px] text-muted-foreground">
                      − 删除
                    </div>
                    <div className="line-through whitespace-pre-wrap opacity-70">
                      {summarizeBlock(entry.block)}
                    </div>
                  </div>
                );
              }
              return (
                <div
                  key={idx}
                  className="space-y-2 rounded-lg border border-border bg-card p-3"
                >
                  <div className="text-[11px] text-muted-foreground">
                    ~ 修改
                  </div>
                  <div className="rounded bg-muted/40 p-2 line-through whitespace-pre-wrap opacity-70">
                    {summarizeBlock(entry.before)}
                  </div>
                  <div className="rounded bg-muted/40 p-2 whitespace-pre-wrap">
                    {summarizeBlock(entry.after)}
                  </div>
                </div>
              );
            })}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

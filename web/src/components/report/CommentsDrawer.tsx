import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  ArchiveIcon,
  CheckCircle2Icon,
  GitCompareArrowsIcon,
  MessageSquareIcon,
  RotateCcwIcon,
  SparklesIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import { getDocumentDiff } from "@/api/quire";
import { Badge } from "@/components/ui/badge";
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

type CommentFilter = "open" | "addressed" | "archived";

export interface CommentsPanelProps {
  docId: string;
  threads: CommentThread[];
  activeThreadId?: string | null;
  readOnly?: boolean;
  onClose?: () => void;
  onSelectThread?: (thread: CommentThread) => void;
  onSelectBlock?: (blockId: string) => void;
  onReply?: (threadId: string, body: string) => Promise<void>;
  onUpdateStatus?: (threadId: string, status: CommentStatus) => Promise<void>;
  onAskAgent?: (threads: CommentThread[]) => void;
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

function statusLabel(thread: CommentThread): string {
  if (thread.status === "open") return "待处理";
  if (thread.status === "addressed") {
    return `Agent 已处理 (rev ${thread.addressed_rev ?? "?"})`;
  }
  return "已归档";
}

export function CommentsPanel({
  docId,
  threads,
  activeThreadId,
  readOnly = false,
  onClose,
  onSelectThread,
  onSelectBlock,
  onReply,
  onUpdateStatus,
  onAskAgent,
}: CommentsPanelProps) {
  const [replyByThread, setReplyByThread] = useState<Record<string, string>>(
    {},
  );
  const [filter, setFilter] = useState<CommentFilter>("open");
  const [busy, setBusy] = useState(false);
  const [diffRange, setDiffRange] = useState<{
    fromRev: number;
    toRev: number;
    blockId: string;
  } | null>(null);

  const counts = useMemo(
    () => ({
      open: threads.filter((thread) => thread.status === "open").length,
      addressed: threads.filter((thread) => thread.status === "addressed")
        .length,
      archived: threads.filter((thread) => thread.status === "archived").length,
    }),
    [threads],
  );
  const openThreads = useMemo(
    () => threads.filter((thread) => thread.status === "open"),
    [threads],
  );
  const filtered = useMemo(() => {
    const base = threads.filter((thread) => thread.status === filter);
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

  return (
    <>
      <div className="flex h-full min-h-0 flex-col bg-background">
        <div className="flex shrink-0 items-center gap-1 border-b px-3 py-2">
          <div className="flex items-center gap-1.5 text-xs font-semibold">
            <MessageSquareIcon className="size-3.5 text-amber-600 dark:text-amber-400" />
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
                ["addressed", "已处理", counts.addressed],
                ["archived", "已归档", counts.archived],
              ] as const
            ).map(([value, label, count]) => (
              <Button
                key={value}
                type="button"
                size="xs"
                variant={filter === value ? "default" : "ghost"}
                className="h-7 px-1.5 text-[10px]"
                onClick={() => setFilter(value)}
              >
                {label}
                <span className="ml-0.5 tabular-nums opacity-70">{count}</span>
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

        {!readOnly && openThreads.length > 0 && (
          <div className="shrink-0 border-b bg-amber-500/[0.04] px-3 py-2">
            <Button
              type="button"
              size="xs"
              variant="outline"
              className="w-full justify-center gap-1.5 border-amber-500/40 text-amber-900 hover:bg-amber-500/10 dark:text-amber-200"
              onClick={() => onAskAgent?.(openThreads)}
            >
              <SparklesIcon className="size-3.5" />
              全部交给 AI 处理 ({openThreads.length})
            </Button>
          </div>
        )}

        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto overscroll-contain p-3">
          {filtered.length === 0 ? (
            <div className="py-12 text-center text-xs text-muted-foreground">
              {filter === "open"
                ? "暂无待处理批注"
                : filter === "addressed"
                  ? "暂无已处理批注"
                  : "暂无已归档批注"}
            </div>
          ) : (
            filtered.map((thread) => {
              const active = thread.thread_id === activeThreadId;
              const latestMessage =
                thread.messages[thread.messages.length - 1] ?? null;
              const latestAgentMessage =
                [...thread.messages]
                  .reverse()
                  .find((message) => message.author_type === "agent") ?? null;
              const addressedSummary =
                latestAgentMessage ?? latestMessage ?? null;
              return (
                <div
                  key={thread.thread_id}
                  id={`comment-thread-${thread.thread_id}`}
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
                  className={`cursor-pointer space-y-2 rounded-xl border p-3 text-xs transition-colors ${
                    active
                      ? "border-amber-500/60 bg-amber-500/[0.06] ring-1 ring-amber-500/30"
                      : "bg-card hover:bg-muted/30"
                  }`}
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
                      {statusLabel(thread)}
                    </Badge>
                    {thread.anchor_detached ? (
                      <Badge variant="destructive" className="gap-1">
                        <Trash2Icon className="size-3" />
                        原块已删除
                      </Badge>
                    ) : (
                      <button
                        type="button"
                        onClick={(event) => {
                          event.stopPropagation();
                          onSelectBlock?.(thread.block_id);
                        }}
                        className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] hover:underline"
                      >
                        #{thread.block_id}
                      </button>
                    )}
                    {thread.addressed_rev !== undefined &&
                      thread.addressed_rev !== null && (
                        <Button
                          type="button"
                          size="xs"
                          variant="ghost"
                          className="ml-auto h-6 gap-1 px-1.5 text-[11px]"
                          onClick={(event) => {
                            event.stopPropagation();
                            setDiffRange({
                              fromRev: thread.created_rev,
                              toRev: thread.addressed_rev ?? thread.created_rev,
                              blockId: thread.block_id,
                            });
                          }}
                        >
                          <GitCompareArrowsIcon className="size-3" />
                          <span>
                            查看修订 ({thread.created_rev}→
                            {thread.addressed_rev})
                          </span>
                        </Button>
                      )}
                  </div>

                  {thread.quote && (
                    <div className="rounded border-l-2 border-amber-500 bg-amber-500/10 px-2 py-1 text-[11px]">
                      划词：“{thread.quote.exact}”
                    </div>
                  )}

                  {thread.anchor_snapshot && (
                    <blockquote className="border-l-2 border-muted-foreground/30 pl-2 text-[11px] text-muted-foreground italic">
                      {thread.anchor_snapshot}
                    </blockquote>
                  )}

                  {thread.status === "addressed" && addressedSummary ? (
                    <div className="space-y-2">
                      {thread.messages[0] && (
                        <div className="rounded-lg bg-muted/40 p-2">
                          <div className="mb-0.5 text-[10px] text-muted-foreground">
                            原始批注 · {thread.messages[0].author}
                          </div>
                          <p className="whitespace-pre-wrap text-foreground">
                            {thread.messages[0].body}
                          </p>
                        </div>
                      )}
                      <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/[0.06] p-2">
                        <div className="mb-1 flex items-center gap-1 text-[10px] font-medium text-emerald-700 dark:text-emerald-300">
                          <CheckCircle2Icon className="size-3" />
                          最新修改说明
                        </div>
                        <p className="whitespace-pre-wrap text-foreground">
                          {addressedSummary.body}
                        </p>
                      </div>
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {thread.messages.map((message) => (
                        <div
                          key={message.message_id}
                          className="rounded-lg bg-muted/40 p-2"
                        >
                          <div className="mb-0.5 flex items-center justify-between text-[11px] text-muted-foreground">
                            <span className="font-medium text-foreground">
                              {message.author} (
                              {message.author_type === "agent"
                                ? "Agent"
                                : "人类"}
                              )
                            </span>
                          </div>
                          <p className="whitespace-pre-wrap text-foreground">
                            {message.body}
                          </p>
                        </div>
                      ))}
                    </div>
                  )}

                  {!readOnly && thread.status !== "archived" && (
                    <div className="space-y-1.5 pt-1">
                      <div className="flex gap-1.5">
                        <Textarea
                          rows={1}
                          className="min-h-8 text-xs"
                          placeholder="回复..."
                          value={replyByThread[thread.thread_id] ?? ""}
                          onClick={(event) => event.stopPropagation()}
                          onChange={(event) => {
                            const value = event.target.value;
                            setReplyByThread((prev) => ({
                              ...prev,
                              [thread.thread_id]: value,
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
                          onClick={(event) => {
                            event.stopPropagation();
                            const body = (
                              replyByThread[thread.thread_id] ?? ""
                            ).trim();
                            if (!body || !onReply) return;
                            void (async () => {
                              setBusy(true);
                              try {
                                await onReply(thread.thread_id, body);
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
                  )}

                  <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
                    {!readOnly && thread.status === "open" && onAskAgent && (
                      <Button
                        type="button"
                        size="xs"
                        variant="secondary"
                        className="gap-1"
                        disabled={busy}
                        onClick={(event) => {
                          event.stopPropagation();
                          onAskAgent([thread]);
                        }}
                      >
                        <SparklesIcon className="size-3" />
                        交给 AI 修订
                      </Button>
                    )}
                    {!readOnly &&
                      thread.status === "addressed" &&
                      onUpdateStatus && (
                        <Button
                          type="button"
                          size="xs"
                          variant="default"
                          className="gap-1 bg-emerald-600 hover:bg-emerald-700"
                          disabled={busy}
                          onClick={(event) => {
                            event.stopPropagation();
                            void (async () => {
                              setBusy(true);
                              try {
                                await onUpdateStatus(
                                  thread.thread_id,
                                  "archived",
                                );
                              } finally {
                                setBusy(false);
                              }
                            })();
                          }}
                        >
                          <CheckCircle2Icon className="size-3" />
                          确认归档
                        </Button>
                      )}
                    {!readOnly &&
                      thread.status === "open" &&
                      onUpdateStatus && (
                        <Button
                          type="button"
                          size="xs"
                          variant="ghost"
                          className="gap-1 text-muted-foreground"
                          disabled={busy}
                          onClick={(event) => {
                            event.stopPropagation();
                            void onUpdateStatus(thread.thread_id, "archived");
                          }}
                        >
                          <ArchiveIcon className="size-3" />
                          直接归档
                        </Button>
                      )}
                    {!readOnly &&
                      thread.status === "archived" &&
                      onUpdateStatus && (
                        <Button
                          type="button"
                          size="xs"
                          variant="ghost"
                          className="gap-1"
                          disabled={busy}
                          onClick={(event) => {
                            event.stopPropagation();
                            void onUpdateStatus(thread.thread_id, "open");
                          }}
                        >
                          <RotateCcwIcon className="size-3" />
                          重新打开
                        </Button>
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
              修订对比 (rev {diffRange?.fromRev} → rev {diffRange?.toRev})
            </DialogTitle>
          </DialogHeader>
          <div className="max-h-[60vh] space-y-3 overflow-y-auto text-xs">
            {diffQuery.isLoading && (
              <div className="py-8 text-center text-muted-foreground">
                正在加载版本差异...
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
                    className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3"
                  >
                    <div className="mb-1 font-mono text-[11px] text-emerald-700 dark:text-emerald-300">
                      + 新增 #{entry.block.id}
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
                    className="rounded-lg border border-destructive/30 bg-destructive/10 p-3"
                  >
                    <div className="mb-1 font-mono text-[11px] text-destructive">
                      - 删除 #{entry.block.id}
                    </div>
                    <div className="line-through whitespace-pre-wrap opacity-80">
                      {summarizeBlock(entry.block)}
                    </div>
                  </div>
                );
              }
              return (
                <div
                  key={idx}
                  className="space-y-2 rounded-lg border border-amber-500/30 bg-amber-500/5 p-3"
                >
                  <div className="font-mono text-[11px] text-amber-700 dark:text-amber-300">
                    ~ 修改 #{entry.block_id}
                  </div>
                  <div className="rounded bg-destructive/10 p-2 line-through whitespace-pre-wrap opacity-80">
                    {summarizeBlock(entry.before)}
                  </div>
                  <div className="rounded bg-emerald-500/10 p-2 whitespace-pre-wrap">
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

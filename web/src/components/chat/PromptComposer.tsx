import { useEffect, useRef, useState } from "react";
import {
  ArrowUpIcon,
  CoinsIcon,
  FileIcon,
  Minimize2Icon,
  PlusIcon,
  QuoteIcon,
  RotateCcwIcon,
  UploadCloudIcon,
  XIcon,
} from "lucide-react";
import { toast } from "sonner";
import {
  Attachment,
  AttachmentAction,
  AttachmentContent,
  AttachmentMedia,
  AttachmentTitle,
} from "@/components/ui/attachment";
import { Button } from "@/components/ui/button";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "@/components/ui/hover-card";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupTextarea,
} from "@/components/ui/input-group";
import { Progress } from "@/components/ui/progress";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type {
  ActiveRunStatus,
  ContextUsage,
  SessionCommentRef,
} from "@/contracts/session-protocol";
import { errorMessage } from "@/lib/errors";
import { useImeCompositionGuard } from "@/lib/ime";
import { selectionRangeLabel } from "@/components/report/reportSelection";

export interface PendingAttachment {
  localId: string;
  file: File;
  attachmentId?: string;
  status: "idle" | "uploading" | "done" | "error";
}

export interface QuotedChatSnippet {
  blockId: string;
  endBlockId?: string;
  blockIds?: string[];
  exact: string;
}

export interface PromptComposerProps {
  placeholder?: string;
  runStatus?: ActiveRunStatus;
  compacting?: boolean;
  contextUsage?: ContextUsage;
  quotedSnippet?: QuotedChatSnippet | null;
  pendingCommentRefs?: SessionCommentRef[];
  prefill?: { key: string; text: string } | null;
  onClearQuote?: () => void;
  onRemoveCommentRef?: (threadId: string) => void;
  onClearCommentRefs?: () => void;
  disabled?: boolean;
  onSend: (
    text: string,
    files: File[],
    commentRefs: SessionCommentRef[],
  ) => Promise<void>;
  onRetry?: () => Promise<void>;
  onCompact?: () => Promise<void>;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

export function formatSessionCostRmb(costRmb: number): string {
  const safe = Number.isFinite(costRmb) && costRmb > 0 ? costRmb : 0;
  if (safe === 0) {
    return "¥0.0000";
  }
  if (safe < 0.0001) {
    return `¥${safe.toFixed(6)}`;
  }
  if (safe < 1) {
    return `¥${safe.toFixed(4)}`;
  }
  return `¥${safe.toFixed(3)}`;
}

/**
 * Smooth "gas-pump odometer" interpolation hook. When the session's cumulative
 * cost increases (from main agent or subagent turns), it smoothly rolls the
 * displayed value at 60fps over ~560ms with cubic ease-out, respecting
 * `prefers-reduced-motion`.
 */
function useGasPumpCostOdometer(targetCostRmb: number | null | undefined): {
  displayedCost: number;
  isRolling: boolean;
} {
  const target =
    typeof targetCostRmb === "number" &&
    Number.isFinite(targetCostRmb) &&
    targetCostRmb >= 0
      ? targetCostRmb
      : 0;
  const [displayedCost, setDisplayedCost] = useState(target);
  const [isRolling, setIsRolling] = useState(false);
  const currentRef = useRef(target);

  useEffect(() => {
    const startValue = currentRef.current;
    const diff = target - startValue;
    if (Math.abs(diff) < 1e-8) {
      currentRef.current = target;
      return undefined;
    }
    const reducedMotion =
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (
      reducedMotion ||
      typeof window === "undefined" ||
      typeof window.requestAnimationFrame !== "function"
    ) {
      currentRef.current = target;
      // oxlint-disable-next-line react/set-state-in-effect -- SSR/reduced-motion immediate sync
      setDisplayedCost(target);
      return undefined;
    }

    const durationMs = 560;
    const startedAt = window.performance.now();
    let frameId = 0;
    setIsRolling(true);

    const step = (now: number) => {
      const elapsed = Math.max(0, now - startedAt);
      const progress = Math.min(1, elapsed / durationMs);
      const eased = 1 - (1 - progress) ** 3;
      const nextValue = startValue + diff * eased;
      currentRef.current = nextValue;
      setDisplayedCost(nextValue);
      if (progress < 1) {
        frameId = window.requestAnimationFrame(step);
      } else {
        currentRef.current = target;
        setDisplayedCost(target);
        setIsRolling(false);
      }
    };

    frameId = window.requestAnimationFrame(step);
    return () => {
      window.cancelAnimationFrame(frameId);
      setIsRolling(false);
    };
  }, [target]);

  return { displayedCost, isRolling };
}

function ContextRing({ pct }: { pct: number }) {
  const radius = 6;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference - (pct / 100) * circumference;
  return (
    <svg viewBox="0 0 16 16" className="size-3.5 -rotate-90" aria-hidden="true">
      <circle
        cx="8"
        cy="8"
        r={radius}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        className="opacity-20"
      />
      <circle
        cx="8"
        cy="8"
        r={radius}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeDasharray={circumference}
        strokeDashoffset={offset}
        strokeLinecap="round"
        className={
          pct >= 85
            ? "text-destructive"
            : pct >= 65
              ? "text-amber-500"
              : "text-[#1D4ED8]"
        }
      />
    </svg>
  );
}

export function PromptComposer({
  placeholder = "输入问题或修改要求（Shift + Enter 换行，支持拖拽或粘贴附件）…",
  runStatus = "idle",
  compacting = false,
  contextUsage,
  quotedSnippet,
  pendingCommentRefs = [],
  prefill,
  onClearQuote,
  onRemoveCommentRef,
  onClearCommentRefs,
  disabled = false,
  onSend,
  onRetry,
  onCompact,
}: PromptComposerProps) {
  const [text, setText] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const [sending, setSending] = useState(false);
  const [compactingRequest, setCompactingRequest] = useState(false);
  const [dragging, setDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const ime = useImeCompositionGuard();

  useEffect(() => {
    if (prefill) {
      // oxlint-disable-next-line react/set-state-in-effect -- draft refill is driven by an outside queue action
      setText(prefill.text);
      textareaRef.current?.focus();
    }
  }, [prefill]);

  useEffect(() => {
    if (quotedSnippet) {
      textareaRef.current?.focus();
    }
  }, [quotedSnippet]);

  const isRunning = runStatus === "running";
  const canRetry = runStatus === "interrupted" || runStatus === "failed";
  const composerPlaceholder = isRunning
    ? "任务进行中，输入后按 Enter 排队（Shift + Enter 换行）…"
    : placeholder;

  const used = contextUsage?.context_used ?? 0;
  const limit = contextUsage?.context_limit ?? 0;
  const pct = limit > 0 ? Math.min(100, Math.round((used / limit) * 100)) : 0;
  const rawCostRmb = contextUsage?.cost_rmb;
  const hasCost =
    rawCostRmb !== undefined &&
    rawCostRmb !== null &&
    Number.isFinite(rawCostRmb);
  const { displayedCost, isRolling } = useGasPumpCostOdometer(rawCostRmb);

  const submit = async () => {
    const trimmed = text.trim();
    const currentRefs = pendingCommentRefs;
    if (
      (!trimmed &&
        files.length === 0 &&
        !quotedSnippet &&
        currentRefs.length === 0) ||
      sending ||
      disabled
    ) {
      return;
    }
    setSending(true);
    const currentText = trimmed;
    const currentFiles = files;
    const quoteRange = quotedSnippet ? selectionRangeLabel(quotedSnippet) : "";
    const coveredBlocks =
      quotedSnippet?.blockIds && quotedSnippet.blockIds.length > 1
        ? `；覆盖块: ${quotedSnippet.blockIds.join(", ")}`
        : "";
    const baseText = quotedSnippet
      ? `> 引用（${quoteRange}${coveredBlocks}）：${quotedSnippet.exact}\n\n${currentText}`
      : currentText;
    try {
      setText("");
      setFiles([]);
      onClearQuote?.();
      await onSend(baseText, currentFiles, currentRefs);
      if (currentRefs.length > 0) {
        onClearCommentRefs?.();
      }
    } catch (err) {
      setText(currentText);
      setFiles(currentFiles);
      toast.error(errorMessage(err, "发送失败，请重试"));
    } finally {
      setSending(false);
    }
  };

  return (
    <div
      className="relative w-full space-y-2"
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes("Files")) {
          event.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={(event) => {
        const target = event.relatedTarget;
        if (
          !(target instanceof Node) ||
          !event.currentTarget.contains(target)
        ) {
          setDragging(false);
        }
      }}
      onDrop={(event) => {
        event.preventDefault();
        setDragging(false);
        const dropped = Array.from(event.dataTransfer.files);
        if (dropped.length > 0) {
          setFiles((prev) => [...prev, ...dropped]);
        }
      }}
    >
      {dragging && (
        <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center gap-2 rounded-2xl border-2 border-dashed border-primary bg-primary/5 text-xs font-medium text-primary backdrop-blur-[1px]">
          <UploadCloudIcon className="size-4" />
          <span>松开以上传附件</span>
        </div>
      )}

      {pendingCommentRefs.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {pendingCommentRefs.map((ref) => (
            <button
              key={ref.thread_id}
              type="button"
              onClick={() => onRemoveCommentRef?.(ref.thread_id)}
              title={`移除批注：${ref.comment}`}
              aria-label={`移除批注：${ref.comment}`}
              className="inline-flex max-w-full items-center gap-1.5 rounded-full border border-border bg-muted/60 px-2.5 py-1 text-[11px] text-foreground transition-colors hover:bg-muted"
            >
              <QuoteIcon className="size-3 shrink-0 text-muted-foreground" />
              <span className="max-w-[16rem] truncate text-muted-foreground">
                {ref.comment}
              </span>
              <XIcon className="size-3 shrink-0 text-muted-foreground" />
            </button>
          ))}
        </div>
      )}

      {quotedSnippet && (
        <div className="flex items-start justify-between gap-2 rounded-xl border border-border bg-muted/40 px-3 py-2 text-xs">
          <div className="flex min-w-0 items-start gap-1.5">
            <QuoteIcon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
            <div className="min-w-0">
              <div className="text-[10px] font-medium text-muted-foreground">
                引用报告内容
              </div>
              <div className="line-clamp-2 text-muted-foreground">
                “{quotedSnippet.exact}”
              </div>
            </div>
          </div>
          {onClearQuote && (
            <button
              type="button"
              aria-label="取消引用"
              onClick={onClearQuote}
              className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
            >
              <XIcon className="size-3.5" />
            </button>
          )}
        </div>
      )}

      {files.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {files.map((file, idx) => (
            <Attachment key={`${file.name}-${idx}`} size="xs" state="done">
              <AttachmentMedia variant="icon">
                <FileIcon />
              </AttachmentMedia>
              <AttachmentContent>
                <AttachmentTitle>{file.name}</AttachmentTitle>
              </AttachmentContent>
              <AttachmentAction
                aria-label={`移除 ${file.name}`}
                onClick={() =>
                  setFiles((prev) => prev.filter((_, i) => i !== idx))
                }
              >
                <XIcon />
              </AttachmentAction>
            </Attachment>
          ))}
        </div>
      )}

      <InputGroup className="rounded-2xl bg-card shadow-xs">
        <InputGroupTextarea
          ref={textareaRef}
          name="prompt"
          autoComplete="off"
          aria-label={isRunning ? "输入排队消息" : "输入问题或修改要求"}
          value={text}
          placeholder={composerPlaceholder}
          rows={2}
          disabled={disabled || sending}
          className="max-h-40 min-h-12 resize-none px-3.5 pt-3 text-sm"
          onChange={(event) => setText(event.target.value)}
          onCompositionStart={ime.onCompositionStart}
          onCompositionEnd={ime.onCompositionEnd}
          onPaste={(event) => {
            const pastedFiles = Array.from(event.clipboardData.files);
            if (pastedFiles.length > 0) {
              setFiles((prev) => [...prev, ...pastedFiles]);
            }
          }}
          onKeyDown={(event) => {
            if (ime.isSubmitEnter(event)) {
              event.preventDefault();
              void submit();
            }
          }}
        />
        <InputGroupAddon
          align="block-end"
          className="justify-between px-2 pb-2"
        >
          <div className="flex items-center gap-1">
            <input
              ref={fileInputRef}
              type="file"
              multiple
              className="hidden"
              onChange={(event) => {
                const picked = Array.from(event.target.files ?? []);
                if (picked.length > 0) {
                  setFiles((prev) => [...prev, ...picked]);
                }
                event.target.value = "";
              }}
            />
            <InputGroupButton
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label="上传文件或图片"
              disabled={disabled || sending}
              onClick={() => fileInputRef.current?.click()}
            >
              <PlusIcon className="size-4" />
            </InputGroupButton>

            {contextUsage && (limit > 0 || hasCost) && (
              <HoverCard>
                <HoverCardTrigger
                  render={
                    <Button
                      type="button"
                      size="xs"
                      variant="ghost"
                      data-slot="context-usage-trigger"
                      className="gap-1.5 rounded-lg px-2 text-xs text-muted-foreground tabular-nums hover:bg-secondary/80 hover:text-foreground"
                    />
                  }
                >
                  {limit > 0 && (
                    <span className="inline-flex items-center gap-1 font-mono text-[11.5px] tabular-nums">
                      <ContextRing pct={pct} />
                      <span>{pct}%</span>
                    </span>
                  )}
                  {(hasCost || used > 0) && (
                    <>
                      {limit > 0 && (
                        <span className="text-border" aria-hidden="true">
                          ·
                        </span>
                      )}
                      <span
                        data-slot="session-cost-odometer"
                        data-rolling={isRolling ? "true" : "false"}
                        className={`inline-flex items-center gap-1 font-mono text-[11.5px] tabular-nums transition-colors duration-200 ${
                          isRolling
                            ? "font-medium text-emerald-600 dark:text-emerald-400"
                            : "text-muted-foreground"
                        }`}
                      >
                        <CoinsIcon className="size-3 shrink-0 opacity-75" />
                        <span>{formatSessionCostRmb(displayedCost)}</span>
                      </span>
                    </>
                  )}
                </HoverCardTrigger>
                <HoverCardContent className="w-72 space-y-3 rounded-xl border border-border/90 bg-card p-3.5 shadow-md">
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between text-xs font-medium text-foreground">
                      <span>上下文容量</span>
                      <span className="font-mono text-[11px] text-muted-foreground tabular-nums">
                        {formatTokens(used)} / {formatTokens(limit)} ({pct}%)
                      </span>
                    </div>
                    <Progress value={pct} aria-label="Context used" />
                  </div>

                  <div className="grid grid-cols-3 gap-2 rounded-lg border border-border/70 bg-secondary/40 px-2.5 py-2 text-[11px]">
                    <div>
                      <div className="text-muted-foreground">输入 Token</div>
                      <div className="mt-0.5 font-mono font-medium text-foreground tabular-nums">
                        {formatTokens(contextUsage.input_tokens)}
                      </div>
                    </div>
                    <div>
                      <div className="text-muted-foreground">输出 Token</div>
                      <div className="mt-0.5 font-mono font-medium text-foreground tabular-nums">
                        {formatTokens(contextUsage.output_tokens)}
                      </div>
                    </div>
                    <div>
                      <div className="text-muted-foreground">缓存命中</div>
                      <div className="mt-0.5 font-mono font-medium text-emerald-600 tabular-nums dark:text-emerald-400">
                        {formatTokens(contextUsage.cached_tokens)}
                      </div>
                    </div>
                  </div>

                  {(hasCost || used > 0) && (
                    <div className="flex items-center justify-between border-t border-border/60 pt-2 text-xs">
                      <span className="text-muted-foreground">
                        本会话累计消耗
                      </span>
                      <span className="font-mono font-semibold text-foreground tabular-nums">
                        {formatSessionCostRmb(displayedCost)}
                      </span>
                    </div>
                  )}

                  {onCompact && (
                    <Tooltip>
                      <TooltipTrigger
                        render={<span className="block w-full" />}
                      >
                        <Button
                          type="button"
                          size="xs"
                          variant="outline"
                          data-slot="compact-context-button"
                          className="w-full gap-1.5"
                          disabled={
                            isRunning || compacting || compactingRequest
                          }
                          onClick={() => {
                            if (isRunning || compacting || compactingRequest) {
                              return;
                            }
                            void (async () => {
                              setCompactingRequest(true);
                              try {
                                await onCompact();
                              } catch (err) {
                                toast.error(
                                  errorMessage(
                                    err,
                                    "上下文压缩失败，请稍后重试",
                                  ),
                                );
                              } finally {
                                setCompactingRequest(false);
                              }
                            })();
                          }}
                        >
                          <Minimize2Icon className="size-3" />
                          <span>
                            {compacting || compactingRequest
                              ? "正在压缩上下文…"
                              : "压缩上下文 (/compact)"}
                          </span>
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent>
                        {isRunning
                          ? "任务执行中暂不可压缩"
                          : compacting
                            ? "服务端正在压缩上下文"
                            : compactingRequest
                              ? "正在生成结构化交接摘要"
                              : "压缩早期对话历史并释放上下文空间"}
                      </TooltipContent>
                    </Tooltip>
                  )}
                </HoverCardContent>
              </HoverCard>
            )}
          </div>

          <div className="flex items-center gap-1.5">
            {canRetry && onRetry && (
              <Button
                type="button"
                size="xs"
                variant="outline"
                className="gap-1"
                onClick={() => {
                  void onRetry();
                }}
              >
                <RotateCcwIcon className="size-3" />
                <span>重试</span>
              </Button>
            )}
            <Button
              type="button"
              size="icon-sm"
              aria-label={isRunning ? "排队发送消息" : "发送消息"}
              title={isRunning ? "加入等待队列" : "发送消息"}
              disabled={
                disabled ||
                sending ||
                (text.trim().length === 0 &&
                  files.length === 0 &&
                  !quotedSnippet &&
                  pendingCommentRefs.length === 0)
              }
              onClick={() => {
                void submit();
              }}
            >
              <ArrowUpIcon className="size-4" />
            </Button>
          </div>
        </InputGroupAddon>
      </InputGroup>
    </div>
  );
}

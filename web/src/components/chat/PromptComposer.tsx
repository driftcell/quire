import { useEffect, useRef, useState } from "react";
import {
  ArrowUpIcon,
  FileIcon,
  Minimize2Icon,
  PlusIcon,
  QuoteIcon,
  RotateCcwIcon,
  UploadCloudIcon,
  XIcon,
} from "lucide-react";
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
import type {
  ActiveRunStatus,
  ContextUsage,
} from "@/contracts/session-protocol";
import {
  serializeCommentRefs,
  type CommentRefPayload,
} from "@/lib/commentRefXml";

export interface PendingAttachment {
  localId: string;
  file: File;
  attachmentId?: string;
  status: "idle" | "uploading" | "done" | "error";
}

export interface QuotedChatSnippet {
  blockId: string;
  exact: string;
}

export interface PromptComposerProps {
  placeholder?: string;
  runStatus?: ActiveRunStatus;
  contextUsage?: ContextUsage;
  quotedSnippet?: QuotedChatSnippet | null;
  pendingCommentRefs?: CommentRefPayload[];
  prefill?: { key: string; text: string } | null;
  onClearQuote?: () => void;
  onRemoveCommentRef?: (threadId: string) => void;
  onClearCommentRefs?: () => void;
  disabled?: boolean;
  onSend: (text: string, files: File[]) => Promise<void>;
  onRetry?: () => Promise<void>;
  onCompact?: () => Promise<void>;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
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
              : "text-primary"
        }
      />
    </svg>
  );
}

export function PromptComposer({
  placeholder = "Shift + Enter 换行 · 支持拖拽或粘贴截图",
  runStatus = "idle",
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
  const [dragging, setDragging] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (prefill) {
      // Recalling a queued message is an external event that refills the
      // composer; there is no render-time equivalent for the focus handoff.
      // oxlint-disable-next-line react/set-state-in-effect -- draft refill is driven by an outside queue action
      setText(prefill.text);
      setSendError(null);
      textareaRef.current?.focus();
    }
  }, [prefill]);

  const isRunning = runStatus === "running";
  const canRetry = runStatus === "interrupted" || runStatus === "failed";

  const used = contextUsage?.context_used ?? 0;
  const limit = contextUsage?.context_limit ?? 0;
  const pct = limit > 0 ? Math.min(100, Math.round((used / limit) * 100)) : 0;

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
    setSendError(null);
    const currentText = trimmed;
    const currentFiles = files;
    const baseText = quotedSnippet
      ? `> 引用（#${quotedSnippet.blockId}）：${quotedSnippet.exact}\n\n${currentText}`
      : currentText;
    const payloadText =
      currentRefs.length > 0
        ? serializeCommentRefs(currentRefs, baseText)
        : baseText;
    try {
      setText("");
      setFiles([]);
      onClearQuote?.();
      await onSend(payloadText, currentFiles);
      if (currentRefs.length > 0) {
        onClearCommentRefs?.();
      }
    } catch (err) {
      setText(currentText);
      setFiles(currentFiles);
      setSendError(err instanceof Error ? err.message : "发送失败，请重试");
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

      {sendError && (
        <div className="flex items-center justify-between gap-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          <span className="truncate">{sendError}</span>
          <button
            type="button"
            aria-label="关闭错误提示"
            onClick={() => setSendError(null)}
            className="shrink-0 rounded p-0.5 hover:bg-destructive/15"
          >
            <XIcon className="size-3.5" />
          </button>
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
                引用报告块 #{quotedSnippet.blockId}
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

      {isRunning && (
        <div className="px-1 text-[11px] text-muted-foreground">
          Agent 正在运行，发送后将进入队列
        </div>
      )}

      <InputGroup className="rounded-2xl bg-card shadow-xs">
        <InputGroupTextarea
          ref={textareaRef}
          value={text}
          placeholder={placeholder}
          rows={2}
          disabled={disabled || sending}
          className="max-h-40 min-h-12 resize-none px-3.5 pt-3 text-sm"
          onChange={(event) => setText(event.target.value)}
          onPaste={(event) => {
            const pastedFiles = Array.from(event.clipboardData.files);
            if (pastedFiles.length > 0) {
              setFiles((prev) => [...prev, ...pastedFiles]);
            }
          }}
          onKeyDown={(event) => {
            if (
              event.key === "Enter" &&
              !event.shiftKey &&
              !event.nativeEvent.isComposing
            ) {
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

            {contextUsage && limit > 0 && (
              <HoverCard>
                <HoverCardTrigger
                  render={
                    <Button
                      type="button"
                      size="xs"
                      variant="ghost"
                      className="gap-1.5 text-xs text-muted-foreground tabular-nums"
                    />
                  }
                >
                  <ContextRing pct={pct} />
                  <span>{pct}%</span>
                </HoverCardTrigger>
                <HoverCardContent className="w-64 space-y-2.5">
                  <div className="flex items-center justify-between text-xs font-medium">
                    <span>上下文占用</span>
                    <span className="tabular-nums">
                      {formatTokens(used)} / {formatTokens(limit)}
                    </span>
                  </div>
                  <Progress value={pct} aria-label="Context used" />
                  <div className="grid grid-cols-3 gap-2 text-[11px] text-muted-foreground">
                    <div>
                      <div>Input</div>
                      <div className="font-medium text-foreground tabular-nums">
                        {formatTokens(contextUsage.input_tokens)}
                      </div>
                    </div>
                    <div>
                      <div>Output</div>
                      <div className="font-medium text-foreground tabular-nums">
                        {formatTokens(contextUsage.output_tokens)}
                      </div>
                    </div>
                    <div>
                      <div>Cached</div>
                      <div className="font-medium text-emerald-600 tabular-nums dark:text-emerald-400">
                        {formatTokens(contextUsage.cached_tokens)}
                      </div>
                    </div>
                  </div>
                  {onCompact && (
                    <Button
                      type="button"
                      size="xs"
                      variant="outline"
                      className="w-full gap-1.5"
                      onClick={() => {
                        void onCompact();
                      }}
                    >
                      <Minimize2Icon className="size-3" />
                      <span>压缩上下文 (/compact)</span>
                    </Button>
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

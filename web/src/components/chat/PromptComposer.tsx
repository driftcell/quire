import { useRef, useState } from "react";
import {
  ArrowUpIcon,
  FileIcon,
  GaugeIcon,
  Minimize2Icon,
  PaperclipIcon,
  RotateCcwIcon,
  SquareIcon,
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

export interface PendingAttachment {
  localId: string;
  file: File;
  attachmentId?: string;
  status: "idle" | "uploading" | "done" | "error";
}

export interface PromptComposerProps {
  placeholder?: string;
  runStatus?: ActiveRunStatus;
  contextUsage?: ContextUsage;
  disabled?: boolean;
  onSend: (text: string, files: File[]) => Promise<void>;
  onInterrupt?: () => Promise<void>;
  onRetry?: () => Promise<void>;
  onCompact?: () => Promise<void>;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

export function PromptComposer({
  placeholder = "描述你想生成的报告主题、结构或修改意见...",
  runStatus = "idle",
  contextUsage,
  disabled = false,
  onSend,
  onInterrupt,
  onRetry,
  onCompact,
}: PromptComposerProps) {
  const [text, setText] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const [sending, setSending] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const isRunning = runStatus === "running";
  const canRetry = runStatus === "interrupted" || runStatus === "failed";

  const used = contextUsage?.context_used ?? 0;
  const limit = contextUsage?.context_limit ?? 0;
  const pct = limit > 0 ? Math.min(100, Math.round((used / limit) * 100)) : 0;

  const submit = async () => {
    const trimmed = text.trim();
    if ((!trimmed && files.length === 0) || sending || disabled) {
      return;
    }
    setSending(true);
    try {
      const currentFiles = files;
      setText("");
      setFiles([]);
      await onSend(trimmed, currentFiles);
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="w-full space-y-2">
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
          value={text}
          placeholder={placeholder}
          rows={2}
          disabled={disabled || sending}
          className="max-h-40 min-h-12 resize-none px-3.5 pt-3 text-sm"
          onChange={(event) => setText(event.target.value)}
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
              <PaperclipIcon className="size-4" />
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
                  <GaugeIcon className="size-3.5" />
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
            {isRunning && onInterrupt ? (
              <Button
                type="button"
                size="icon-sm"
                variant="destructive"
                aria-label="停止生成"
                onClick={() => {
                  void onInterrupt();
                }}
              >
                <SquareIcon className="size-3.5 fill-current" />
              </Button>
            ) : (
              <Button
                type="button"
                size="icon-sm"
                aria-label="发送消息"
                disabled={
                  disabled ||
                  sending ||
                  (text.trim().length === 0 && files.length === 0)
                }
                onClick={() => {
                  void submit();
                }}
              >
                <ArrowUpIcon className="size-4" />
              </Button>
            )}
          </div>
        </InputGroupAddon>
      </InputGroup>
    </div>
  );
}

import { memo, useEffect, useRef, useState } from "react";
import { BrainIcon, ChevronRightIcon } from "lucide-react";
import { cn } from "cn";
import { useSmoothStreamText } from "@/hooks/useSmoothStreamText";

export interface ThinkingTraceProps {
  content: string;
  isStreaming: boolean;
  isActive?: boolean;
  /** Live main-agent thinking opens automatically; subagents stay collapsed. */
  autoExpand?: boolean;
  /** Bound large hydration bursts while retaining smooth incremental deltas. */
  maxSmoothBacklog?: number;
  /** Server timestamp when this thinking segment started. */
  startedAtMs?: number;
  /** Server-settled duration for a completed thinking segment. */
  durationMs?: number;
}

function formatDurationSeconds(totalSeconds: number): string {
  if (totalSeconds < 60) {
    return `${totalSeconds}秒`;
  }
  const mins = Math.floor(totalSeconds / 60);
  const secs = totalSeconds % 60;
  return secs > 0 ? `${mins}分${secs}秒` : `${mins}分`;
}

export const ThinkingTrace = memo(function ThinkingTrace({
  content,
  isStreaming,
  isActive = false,
  autoExpand = true,
  maxSmoothBacklog,
  startedAtMs,
  durationMs,
}: ThinkingTraceProps) {
  const [userOpen, setUserOpen] = useState<boolean | undefined>(undefined);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);

  const expanded = userOpen ?? (autoExpand && (isStreaming || isActive));
  const { text: smoothedContent } = useSmoothStreamText(
    content,
    isStreaming && expanded,
    maxSmoothBacklog,
  );

  const hasContent = content.trim().length > 0;
  const elapsedSeconds =
    isStreaming && startedAtMs !== undefined
      ? Math.max(0, Math.floor((nowMs - startedAtMs) / 1000))
      : 0;
  const settledSeconds =
    durationMs !== undefined && durationMs > 0
      ? Math.max(1, Math.ceil(durationMs / 1000))
      : 0;
  const shownSeconds = isStreaming ? elapsedSeconds : settledSeconds;
  const hasDuration = shownSeconds > 0;
  const title = isStreaming
    ? "正在深度思考"
    : hasDuration
      ? "思考用时"
      : "思考过程";

  useEffect(() => {
    if (!isStreaming || startedAtMs === undefined) {
      return undefined;
    }
    const timer = window.setInterval(() => {
      setNowMs(Date.now());
    }, 1000);
    return () => window.clearInterval(timer);
  }, [isStreaming, startedAtMs]);

  useEffect(() => {
    if (!expanded || !isStreaming || !stickToBottomRef.current) {
      return;
    }
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [expanded, isStreaming, smoothedContent]);

  if (!hasContent) {
    return null;
  }

  return (
    <div
      data-slot="thinking-trace"
      data-duration-ms={durationMs}
      className="w-full min-w-0"
    >
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setUserOpen((prev) => !(prev ?? expanded))}
        className="group/thinking inline-flex h-6 max-w-full items-center gap-2 rounded-md text-left text-xs text-muted-foreground transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <span className="flex size-4 shrink-0 items-center justify-center">
          <BrainIcon
            className={cn(
              "size-3.5",
              isStreaming ? "text-primary animate-pulse" : "text-muted-foreground",
            )}
          />
        </span>
        <span
          className={cn(
            "font-medium",
            isStreaming && "text-foreground",
          )}
        >
          {title}
        </span>
        {hasDuration && (
          <span className="tabular-nums text-[11px] text-muted-foreground/75">
            {formatDurationSeconds(shownSeconds)}
          </span>
        )}
        <ChevronRightIcon
          className={cn(
            "size-3.5 shrink-0 text-muted-foreground/70 transition-transform duration-200",
            expanded && "rotate-90",
          )}
        />
      </button>

      <div
        className={cn(
          "grid transition-[grid-template-rows,opacity] duration-200 ease-out",
          expanded ? "mt-1.5 grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
        )}
      >
        <div className="min-h-0 overflow-hidden">
          <div
            ref={scrollRef}
            onScroll={(event) => {
              const el = event.currentTarget;
              const distanceToBottom =
                el.scrollHeight - el.scrollTop - el.clientHeight;
              stickToBottomRef.current = distanceToBottom < 24;
            }}
            className="no-scrollbar max-h-36 overflow-y-auto rounded-r-lg border-l-2 border-primary/25 bg-muted/30 px-3 py-2 text-xs leading-5 text-muted-foreground"
          >
            <div className="whitespace-pre-wrap break-words">
              {smoothedContent}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});

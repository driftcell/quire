import { useEffect, useState } from "react";
import { BrainIcon, LoaderCircleIcon } from "lucide-react";
import {
  MessageScroller,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from "@/components/ui/message-scroller";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";

export interface ThinkingTraceProps {
  content: string;
  isStreaming: boolean;
  isActive?: boolean;
  defaultOpen?: boolean;
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

export function ThinkingTrace({
  content,
  isStreaming,
  isActive = false,
  defaultOpen,
  startedAtMs,
  durationMs,
}: ThinkingTraceProps) {
  const [open, setOpen] = useState<boolean | undefined>(defaultOpen);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const hasContent = content.trim().length > 0;
  const expanded = open ?? (isStreaming || isActive);
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
    ? "正在思考"
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

  if (!hasContent) {
    return null;
  }

  return (
    <Trace data-slot="thinking-trace" data-duration-ms={durationMs}>
      <TraceCollapsible open={expanded} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger>
          <TraceLine
            status={isStreaming ? "running" : undefined}
            className="h-6 text-xs leading-5"
          >
            <TraceIcon>
              {isStreaming ? (
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              ) : (
                <BrainIcon className="size-3.5" />
              )}
            </TraceIcon>
            <TraceTitle>{title}</TraceTitle>
            {hasDuration && (
              <TraceDescription className="tabular-nums">
                {formatDurationSeconds(shownSeconds)}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1 pl-6">
          <MessageScrollerProvider
            autoScroll
            defaultScrollPosition="end"
            scrollEdgeThreshold={24}
          >
            <MessageScroller className="max-h-32">
              <MessageScrollerViewport className="scroll-fade pr-2 text-xs leading-5 text-muted-foreground [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                <MessageScrollerContent className="gap-0">
                  <MessageScrollerItem messageId="thinking-content">
                    <div className="whitespace-pre-wrap break-words">
                      {content}
                    </div>
                  </MessageScrollerItem>
                </MessageScrollerContent>
              </MessageScrollerViewport>
            </MessageScroller>
          </MessageScrollerProvider>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

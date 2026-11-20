import { useEffect, useRef, useState } from "react";
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
  /** Persisted run duration, used when replaying a historical trace. */
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
  durationMs,
}: ThinkingTraceProps) {
  const [open, setOpen] = useState<boolean | undefined>(defaultOpen);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const startedAtRef = useRef<number | null>(null);

  const hasContent = content.trim().length > 0;
  const expanded = open ?? (isStreaming || isActive);
  const historicalSeconds =
    durationMs !== undefined && durationMs > 0
      ? Math.max(1, Math.ceil(durationMs / 1000))
      : 0;
  const shownSeconds = elapsedSeconds > 0 ? elapsedSeconds : historicalSeconds;
  const hasDuration = shownSeconds > 0;
  const title = isStreaming
    ? "正在思考"
    : hasDuration
      ? "思考用时"
      : "思考过程";

  useEffect(() => {
    if (!isStreaming) {
      return undefined;
    }
    if (startedAtRef.current === null) {
      startedAtRef.current = Date.now();
    }
    const timer = window.setInterval(() => {
      if (startedAtRef.current !== null) {
        setElapsedSeconds(
          Math.max(0, Math.floor((Date.now() - startedAtRef.current) / 1000)),
        );
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [isStreaming]);

  if (!hasContent) {
    return null;
  }

  return (
    <Trace>
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

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { BrainIcon, LoaderCircleIcon } from "lucide-react";
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

const STICK_TO_BOTTOM_THRESHOLD_PX = 24;

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
  const scrollRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);

  const hasContent = content.trim().length > 0;
  const expanded = open ?? (isStreaming || isActive);
  const historicalSeconds =
    durationMs !== undefined && durationMs > 0
      ? Math.max(1, Math.ceil(durationMs / 1000))
      : 0;
  const shownSeconds = elapsedSeconds > 0 ? elapsedSeconds : historicalSeconds;

  useEffect(() => {
    if (!isStreaming) {
      return undefined;
    }
    stickToBottomRef.current = true;
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

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !hasContent || !expanded || !isStreaming) {
      return;
    }
    if (stickToBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [expanded, hasContent, isStreaming]);

  useEffect(() => {
    const inner = innerRef.current;
    const el = scrollRef.current;
    if (!inner || !el || !hasContent || !expanded || !isStreaming) {
      return undefined;
    }
    const observer = new ResizeObserver(() => {
      if (stickToBottomRef.current) {
        el.scrollTop = el.scrollHeight;
      }
    });
    observer.observe(inner);
    return () => observer.disconnect();
  }, [expanded, hasContent, isStreaming]);

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
            <TraceTitle>{isStreaming ? "正在思考" : "思考用时"}</TraceTitle>
            {shownSeconds > 0 && (
              <TraceDescription className="tabular-nums">
                {formatDurationSeconds(shownSeconds)}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1 pl-6">
          <div
            ref={scrollRef}
            onScroll={() => {
              const el = scrollRef.current;
              if (!el) return;
              stickToBottomRef.current =
                el.scrollHeight - el.scrollTop - el.clientHeight <=
                STICK_TO_BOTTOM_THRESHOLD_PX;
            }}
            className="scroll-fade max-h-32 overflow-y-auto pr-2 text-xs leading-5 text-muted-foreground [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
          >
            <div ref={innerRef} className="whitespace-pre-wrap break-words">
              {content}
            </div>
          </div>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

import { useEffect, useRef, useState } from "react";
import { BrainIcon } from "lucide-react";
import { Streamdown } from "streamdown";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";
import { cn } from "@/lib/utils";

export interface ThinkingTraceProps {
  content: string;
  isStreaming: boolean;
}

export function ThinkingTrace({ content, isStreaming }: ThinkingTraceProps) {
  const [open, setOpen] = useState(isStreaming);
  const [expanded, setExpanded] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!content) {
      return undefined;
    }
    const el = scrollRef.current;
    if (el && isStreaming && !expanded) {
      el.scrollTop = el.scrollHeight;
    }
    return undefined;
  }, [content, isStreaming, expanded]);

  if (!content.trim()) {
    return null;
  }

  return (
    <Trace className="my-1">
      <TraceCollapsible open={open || isStreaming} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger>
          <TraceLine status={isStreaming ? "running" : undefined}>
            <TraceIcon>
              <BrainIcon className="size-3.5" />
            </TraceIcon>
            <TraceTitle>
              {isStreaming ? "Thinking..." : "Thought process"}
            </TraceTitle>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1.5 pl-5">
          <div className="rounded-lg border bg-muted/40 p-2.5 text-xs text-muted-foreground">
            <div
              ref={scrollRef}
              className={cn(
                "prose prose-xs dark:prose-invert max-w-none leading-6 transition-all",
                !expanded && "max-h-[7.5rem] overflow-y-auto",
              )}
            >
              <Streamdown>{content}</Streamdown>
            </div>
            <button
              type="button"
              onClick={() => setExpanded((prev) => !prev)}
              className="mt-1.5 text-[11px] font-medium text-primary/80 hover:text-primary hover:underline"
            >
              {expanded ? "收起至 5 行" : "展开完整思考"}
            </button>
          </div>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

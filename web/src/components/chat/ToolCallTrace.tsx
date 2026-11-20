import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2Icon } from "lucide-react";
import { getToolMeta } from "@/components/chat/toolMeta";
import { getToolCall } from "@/api/runtime";
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

export interface ToolCallTraceItem {
  toolCallId: string;
  toolName: string;
  status: "running" | "completed" | "failed";
  inputPreview: string;
  outputPreview?: string;
  truncated?: boolean;
  durationMs?: number;
}

export interface ToolCallTraceProps {
  sessionId: string;
  item: ToolCallTraceItem;
}

export function ToolCallTrace({ sessionId, item }: ToolCallTraceProps) {
  const [open, setOpen] = useState(false);
  const meta = getToolMeta(item.toolName);
  const Icon = meta.icon;

  const detailQuery = useQuery({
    queryKey: ["tool-call", sessionId, item.toolCallId],
    queryFn: () => getToolCall(sessionId, item.toolCallId),
    enabled: open && item.status !== "running",
    staleTime: 5 * 60_000,
  });

  const lineStatus =
    item.status === "running"
      ? "running"
      : item.status === "failed"
        ? "error"
        : undefined;

  return (
    <Trace
      data-tool-name={item.toolName}
      data-tool-status={item.status}
      data-slot="tool-trace"
    >
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger showChevron={false}>
          <TraceLine status={lineStatus} className="h-6 text-xs leading-5">
            <TraceIcon>
              {item.status === "running" ? (
                <Loader2Icon className="size-3.5 animate-spin" />
              ) : (
                <Icon
                  className={
                    item.status === "failed"
                      ? "size-3.5 text-destructive"
                      : "size-3.5 text-muted-foreground"
                  }
                />
              )}
            </TraceIcon>
            <TraceTitle className="text-xs">{meta.label}</TraceTitle>
            {item.durationMs !== undefined && (
              <TraceDescription className="text-[11px]">
                {item.durationMs}ms
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1.5 pl-5">
          <div className="space-y-2 rounded-lg border bg-muted/30 p-2.5 text-xs">
            <div className="font-mono text-[11px] text-muted-foreground">
              {item.toolName}
            </div>
            <div>
              <div className="mb-1 text-[11px] font-medium text-muted-foreground">
                Input
              </div>
              <pre className="no-scrollbar max-h-40 overflow-auto rounded bg-background/70 p-2 font-mono text-[11px] whitespace-pre-wrap break-all">
                {detailQuery.data
                  ? JSON.stringify(detailQuery.data.input, null, 2)
                  : item.inputPreview}
              </pre>
            </div>
            {(detailQuery.data || item.outputPreview !== undefined) && (
              <div>
                <div className="mb-1 text-[11px] font-medium text-muted-foreground">
                  Output
                </div>
                <pre className="no-scrollbar max-h-48 overflow-auto rounded bg-background/70 p-2 font-mono text-[11px] whitespace-pre-wrap break-all">
                  {detailQuery.data
                    ? JSON.stringify(detailQuery.data.output, null, 2)
                    : item.outputPreview}
                </pre>
              </div>
            )}
            {detailQuery.isLoading && (
              <div className="text-[11px] text-muted-foreground">
                正在加载完整参数与返回值...
              </div>
            )}
          </div>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

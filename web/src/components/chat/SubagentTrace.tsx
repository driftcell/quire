import { LoaderCircleIcon, UsersIcon } from "lucide-react";
import { SubagentOutput } from "@/components/chat/SubagentOutput";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
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

export interface SubagentTraceModel {
  id: string;
  title: string;
  focusTitle?: string;
  status: "running" | "completed" | "failed";
  thinking: string;
  thinkingStreaming: boolean;
  thinkingStartedAtMs?: number;
  thinkingDurationMs?: number;
  tools: ToolCallTraceItem[];
  outputText: string;
  outputStreaming: boolean;
}

/**
 * A subagent is the second and final collapsible layer. Its thinking, tools
 * and output render flat, with no intermediate "执行详情" wrapper.
 */
export function SubagentTrace({
  sessionId,
  model,
}: {
  sessionId: string;
  model: SubagentTraceModel;
}) {
  const lineStatus =
    model.status === "running"
      ? "running"
      : model.status === "failed"
        ? "error"
        : undefined;

  return (
    <Trace
      data-slot="subagent"
      data-subagent-id={model.id}
      data-status={model.status}
    >
      <TraceCollapsible defaultOpen={false}>
        <TraceCollapsibleTrigger data-subagent-trigger="true">
          <TraceLine status={lineStatus} className="h-6 text-xs leading-5">
            <TraceIcon>
              {model.status === "running" ? (
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              ) : (
                <UsersIcon
                  className={
                    model.status === "failed"
                      ? "size-3.5 text-destructive"
                      : "size-3.5 text-muted-foreground"
                  }
                />
              )}
            </TraceIcon>
            <TraceTitle>{model.title}</TraceTitle>
            {model.focusTitle && (
              <TraceDescription className="truncate text-[11px]">
                {model.focusTitle}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
          <ThinkingTrace
            content={model.thinking}
            isStreaming={model.thinkingStreaming}
            startedAtMs={model.thinkingStartedAtMs}
            durationMs={model.thinkingDurationMs}
          />
          <ToolCallList sessionId={sessionId} tools={model.tools} />
          {model.outputText && (
            <SubagentOutput
              text={model.outputText}
              streaming={model.outputStreaming}
            />
          )}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

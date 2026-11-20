import { ListTreeIcon, UsersIcon } from "lucide-react";
import { SubagentOutput } from "@/components/chat/SubagentOutput";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import { toToolTraceItems } from "@/components/chat/traceSegments";
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
import type { RunTraceSubagent } from "@/contracts/session-protocol";

export function HistoricalSubagent({
  sessionId,
  subagent,
}: {
  sessionId: string;
  subagent: RunTraceSubagent;
}) {
  const title = subagent.display_name?.trim() || subagent.agent_name;
  const subTools = toToolTraceItems(subagent.tools);
  const filteredTools = subTools.filter(
    (tool) => tool.toolName !== "describe_task",
  );
  return (
    <Trace>
      <TraceCollapsible defaultOpen={false}>
        <TraceCollapsibleTrigger>
          <TraceLine
            className="h-6 text-xs leading-5"
            status={subagent.status === "failed" ? "error" : undefined}
          >
            <TraceIcon>
              <UsersIcon className="size-3.5" />
            </TraceIcon>
            <TraceTitle>{title}</TraceTitle>
            {subagent.task_focus?.title && (
              <TraceDescription className="truncate text-[11px]">
                {subagent.task_focus.title}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
          <ThinkingTrace
            content={subagent.thinking}
            isStreaming={false}
            defaultOpen={false}
          />
          {filteredTools.length > 0 && (
            <Trace>
              <TraceCollapsible defaultOpen={false}>
                <TraceCollapsibleTrigger>
                  <TraceLine className="h-6 text-xs leading-5">
                    <TraceIcon>
                      <ListTreeIcon className="size-3.5" />
                    </TraceIcon>
                    <TraceTitle>执行详情</TraceTitle>
                    <TraceDescription className="text-[11px]">
                      {filteredTools.length} 个工具
                    </TraceDescription>
                  </TraceLine>
                </TraceCollapsibleTrigger>
                <TraceCollapsibleContent className="mt-1.5 space-y-1.5 border-l pl-4">
                  <ToolCallList sessionId={sessionId} tools={filteredTools} />
                </TraceCollapsibleContent>
              </TraceCollapsible>
            </Trace>
          )}
          {subagent.summary && (
            <SubagentOutput text={subagent.summary} streaming={false} />
          )}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

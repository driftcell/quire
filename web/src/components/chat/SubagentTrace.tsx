import { memo, useState } from "react";
import { ChevronRightIcon } from "lucide-react";
import { cn } from "cn";
import { AgentAvatar, resolveAgentIdentity } from "@/components/chat/agentAvatar";
import { SubagentOutput } from "@/components/chat/SubagentOutput";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";

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
 * Subagent collaboration card with agent avatar, active focus, tool telemetry,
 * and smooth grid-based expandable details.
 */
export const SubagentTrace = memo(function SubagentTrace({
  sessionId,
  model,
}: {
  sessionId: string;
  model: SubagentTraceModel;
}) {
  const [open, setOpen] = useState(false);
  const identity = resolveAgentIdentity(model.title);

  const toolCount = model.tools.length;
  const runningToolCount = model.tools.filter(
    (t) => t.status === "running",
  ).length;
  const telemetrySummary =
    runningToolCount > 0
      ? `执行中 ${runningToolCount}/${toolCount}`
      : toolCount > 0
        ? `${toolCount} 次操作`
        : model.status === "running"
          ? "运行中"
          : model.status === "failed"
            ? "失败"
            : "已完成";

  return (
    <div
      data-slot="subagent"
      data-subagent-id={model.id}
      data-status={model.status}
      className={cn(
        "w-full min-w-0 rounded-lg border transition-colors",
        model.status === "running"
          ? "border-border bg-muted/25"
          : model.status === "failed"
            ? "border-destructive/30 bg-destructive/5"
            : "border-border/60 bg-background",
      )}
    >
      <button
        type="button"
        data-subagent-trigger="true"
        aria-expanded={open}
        onClick={() => setOpen((prev) => !prev)}
        className="flex min-h-8 w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs transition-colors hover:bg-muted/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <AgentAvatar
          actor={model.title}
          size="xs"
          active={model.status === "running"}
        />
        <span className="shrink-0 font-medium text-foreground">
          {identity.label}
        </span>
        {model.focusTitle && (
          <span className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">
            {model.focusTitle}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-1.5 text-[11px] tabular-nums text-muted-foreground">
          {model.status === "running" && (
            <span
              data-slot="subagent-running-dot"
              aria-hidden="true"
              className="size-1.5 shrink-0 animate-pulse rounded-full bg-primary/80"
            />
          )}
          <span
            className={cn(
              model.status === "failed" && "text-destructive",
            )}
          >
            {telemetrySummary}
          </span>
          <ChevronRightIcon
            className={cn(
              "size-3.5 text-muted-foreground/70 transition-transform duration-150",
              open && "rotate-90",
            )}
          />
        </span>
      </button>

      <div
        className={cn(
          "grid transition-[grid-template-rows,opacity] duration-200 ease-out",
          open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
        )}
      >
        <div className="min-h-0 overflow-hidden">
          <div className="space-y-1.5 border-t border-border/50 px-3 py-2">
            <ThinkingTrace
              content={model.thinking}
              isStreaming={model.thinkingStreaming}
              autoExpand={false}
              maxSmoothBacklog={160}
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
          </div>
        </div>
      </div>
    </div>
  );
});

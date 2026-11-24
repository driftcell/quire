"use client";

import type { ComponentProps } from "react";
import { AlertCircleIcon, CheckIcon, Loader2Icon } from "lucide-react";
import { cn } from "@/lib/utils";
import { mono, paper } from "./surfaces";
import { pct } from "../utils/range";

export interface SubagentItem {
  id?: string | undefined;
  name: string;
  model: string;
  status?: "running" | "completed" | "failed" | undefined;
  detail?: string | undefined;
}

export function SubagentList({
  agents,
  completedCount,
  progress,
  showSummary,
  summaryAgent,
  className,
  ...props
}: Omit<
  ComponentProps<"div">,
  | "children"
  | "agents"
  | "completedCount"
  | "progress"
  | "showSummary"
  | "summaryAgent"
> & {
  agents: readonly SubagentItem[];
  completedCount: number;
  progress: readonly number[];
  showSummary: boolean;
  summaryAgent: SubagentItem;
}) {
  return (
    <div
      data-slot="subagent-list"
      className={cn("flex w-full flex-col gap-2", className)}
      {...props}
    >
      {agents.map((agent, index) => {
        const failed = agent.status === "failed";
        const done =
          agent.status !== undefined
            ? agent.status !== "running"
            : index < completedCount;
        const width = progress[index] ?? 0;
        const percentage = pct(width, 100);

        return (
          <div
            key={agent.id ?? `${agent.name}-${index}`}
            data-subagent-pill={agent.id}
            data-status={agent.status ?? (done ? "completed" : "running")}
            className={cn(
              paper,
              "flex flex-col gap-2 rounded-2xl px-3.5 py-2.5",
              failed && "border-destructive/40 bg-destructive/5",
            )}
          >
            <div className="flex items-center gap-2">
              {failed ? (
                <AlertCircleIcon className="text-destructive size-3.5 shrink-0" />
              ) : done ? (
                <CheckIcon className="fade-in zoom-in-90 animate-in size-3.5 shrink-0 text-emerald-500 duration-200" />
              ) : (
                <Loader2Icon className="text-muted-foreground size-3.5 shrink-0 animate-spin motion-reduce:animate-none" />
              )}
              <span className="min-w-0 flex-1 truncate text-[13.5px] text-foreground">
                {agent.name}
              </span>
              <span className={cn(mono, "text-muted-foreground shrink-0")}>
                {agent.model}
              </span>
            </div>
            {agent.detail && (
              <p className="text-muted-foreground truncate pl-5.5 text-xs">
                {agent.detail}
              </p>
            )}
            <span
              data-slot="subagent-progress"
              aria-label={`${agent.name} progress: ${percentage}%`}
              className="bg-foreground/[0.06] inset-ring-border h-[3px] w-full overflow-hidden rounded-full inset-ring forced-colors:outline"
            >
              <span
                className={cn(
                  "block h-full rounded-full transition-[width] duration-700 forced-color-adjust-none",
                  failed
                    ? "bg-destructive/80"
                    : done
                      ? "bg-emerald-500/70"
                      : "bg-foreground/60",
                )}
                style={{ width: `${percentage}%` }}
              />
            </span>
          </div>
        );
      })}
      {showSummary && (
        <div
          className={cn(
            paper,
            "fade-in slide-in-from-bottom-2 animate-in flex flex-col gap-2 rounded-2xl px-3.5 py-2.5 duration-300",
          )}
        >
          <div className="flex items-center gap-2">
            <Loader2Icon className="text-muted-foreground size-3.5 shrink-0 animate-spin motion-reduce:animate-none" />
            <span className="flex-1 truncate text-[13.5px] text-foreground">
              {summaryAgent.name}
            </span>
            <span className={cn(mono, "text-muted-foreground")}>
              {summaryAgent.model}
            </span>
          </div>
          <span
            data-slot="subagent-summary-progress"
            aria-label={`${summaryAgent.name} progress`}
            className="bg-foreground/[0.06] inset-ring-border h-[3px] w-full overflow-hidden rounded-full inset-ring forced-colors:outline"
          >
            <span className="shimmer shimmer-bg block h-full w-full rounded-full motion-reduce:animate-none" />
          </span>
        </div>
      )}
    </div>
  );
}

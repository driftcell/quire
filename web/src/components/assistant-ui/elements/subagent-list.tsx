"use client";

import type { ComponentProps } from "react";
import { AlertCircleIcon, CheckIcon, Loader2Icon, MinusIcon } from "lucide-react";
import { AgentAvatar } from "@/components/chat/agentAvatar";
import { cn } from "@/lib/utils";
import type { AgentPlanStep } from "./agent-plan";
import { mono, paper } from "./surfaces";
import { pct } from "../utils/range";

export interface SubagentItem {
  id?: string | undefined;
  actor?: string | undefined;
  name: string;
  model: string;
  status?: "running" | "completed" | "failed" | undefined;
  detail?: string | undefined;
  todos?: readonly AgentPlanStep[] | undefined;
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
        const todos = agent.todos ?? [];

        return (
          <div
            key={agent.id ?? `${agent.name}-${index}`}
            data-subagent-pill={agent.id}
            data-status={agent.status ?? (done ? "completed" : "running")}
            className={cn(
              paper,
              "flex flex-col gap-2 rounded-xl px-3 py-2.5",
              failed && "border-destructive/40 bg-destructive/5",
            )}
          >
            <div className="flex items-center gap-2">
              <AgentAvatar
                actor={agent.actor ?? agent.name}
                size="xs"
              />
              <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-foreground">
                {agent.name}
              </span>
              <span
                className={cn(
                  mono,
                  "inline-flex shrink-0 items-center gap-1 text-muted-foreground tabular-nums",
                )}
              >
                {failed ? (
                  <AlertCircleIcon className="text-destructive size-3 shrink-0" />
                ) : done ? (
                  <CheckIcon className="size-3 shrink-0 text-emerald-500" />
                ) : (
                  <Loader2Icon className="size-3 shrink-0 animate-spin text-muted-foreground motion-reduce:animate-none" />
                )}
                <span>{agent.model}</span>
              </span>
            </div>

            {todos.length > 0 ? (
              <ul
                data-slot="subagent-todo-list"
                className="flex flex-col gap-1 pl-0.5"
              >
                {todos.map((todo, tIdx) => {
                  const tSkipped = todo.status === "skipped";
                  const tDone =
                    !tSkipped && (done || todo.status === "completed");
                  const tActive =
                    !done && !tSkipped && !tDone && todo.status === "in_progress";
                  return (
                    <li
                      key={todo.id ?? `${todo.label}-${tIdx}`}
                      className="flex items-center gap-1.5 text-xs"
                    >
                      {tSkipped ? (
                        <MinusIcon className="size-2.5 shrink-0 text-muted-foreground/50" />
                      ) : tDone ? (
                        <CheckIcon className="size-3 shrink-0 text-emerald-500" />
                      ) : tActive ? (
                        <Loader2Icon className="size-3 shrink-0 animate-spin text-foreground" />
                      ) : (
                        <span className="mx-0.5 size-1.5 shrink-0 rounded-full border border-foreground/30" />
                      )}
                      <span
                        className={cn(
                          "truncate",
                          tSkipped && "text-muted-foreground/50 line-through",
                          tDone && "text-muted-foreground",
                          tActive && "font-medium text-foreground",
                          !tDone &&
                            !tActive &&
                            !tSkipped &&
                            "text-muted-foreground/75",
                        )}
                      >
                        {todo.label}
                      </span>
                    </li>
                  );
                })}
              </ul>
            ) : (
              agent.detail && (
                <p className="truncate text-xs text-muted-foreground">
                  {agent.detail}
                </p>
              )
            )}

            <span
              data-slot="subagent-progress"
              aria-label={`${agent.name} progress: ${percentage}%`}
              className="bg-foreground/[0.06] h-[2px] w-full overflow-hidden rounded-full"
            >
              <span
                className={cn(
                  "block h-full rounded-full transition-[width] duration-500",
                  failed
                    ? "bg-destructive/80"
                    : done
                      ? "bg-emerald-500/75"
                      : "bg-foreground/65",
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
            "fade-in slide-in-from-bottom-1 animate-in flex flex-col gap-2 rounded-xl px-3 py-2.5 duration-200",
          )}
        >
          <div className="flex items-center gap-2">
            <Loader2Icon className="text-muted-foreground size-3.5 shrink-0 animate-spin motion-reduce:animate-none" />
            <span className="flex-1 truncate text-[13px] font-medium text-foreground">
              {summaryAgent.name}
            </span>
            <span className={cn(mono, "text-muted-foreground")}>
              {summaryAgent.model}
            </span>
          </div>
          <span
            data-slot="subagent-summary-progress"
            aria-label={`${summaryAgent.name} progress`}
            className="bg-foreground/[0.06] h-[2px] w-full overflow-hidden rounded-full"
          >
            <span className="shimmer shimmer-bg block h-full w-full rounded-full motion-reduce:animate-none" />
          </span>
        </div>
      )}
    </div>
  );
}

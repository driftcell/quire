"use client";

import type { ComponentProps } from "react";
import { AlertCircleIcon, CheckIcon, MinusIcon } from "lucide-react";
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
              "flex flex-col gap-1.5 rounded-xl px-3 py-2.5",
              failed && "border-destructive/40 bg-destructive/5",
            )}
          >
            <div className="flex items-center gap-2">
              <AgentAvatar
                actor={agent.actor ?? agent.name}
                size="xs"
              />
              <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-foreground">
                {agent.name}
              </span>
              <span
                className={cn(
                  mono,
                  "inline-flex shrink-0 items-center gap-1.5 text-muted-foreground tabular-nums",
                )}
              >
                {failed ? (
                  <AlertCircleIcon className="size-3 shrink-0 text-destructive" />
                ) : done ? (
                  <CheckIcon className="size-3 shrink-0 text-emerald-500" />
                ) : (
                  <span
                    aria-hidden
                    className="size-1.5 shrink-0 rounded-full bg-[#1D4ED8]"
                  />
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
                        <span
                          aria-hidden
                          className="mx-0.5 size-1.5 shrink-0 rounded-full bg-[#1D4ED8]"
                        />
                      ) : (
                        <span className="mx-0.5 size-1.5 shrink-0 rounded-full border border-foreground/25" />
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
              className="h-[2px] w-full overflow-hidden rounded-full bg-foreground/[0.06]"
            >
              <span
                className={cn(
                  "block h-full rounded-full transition-[width] duration-500",
                  failed
                    ? "bg-destructive/80"
                    : done
                      ? "bg-emerald-500/75"
                      : "bg-[#1D4ED8]/75",
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
            "flex flex-col gap-1.5 rounded-xl px-3 py-2",
          )}
        >
          <div className="flex items-center gap-2">
            <span
              aria-hidden
              className="size-1.5 shrink-0 rounded-full bg-[#1D4ED8]"
            />
            <span className="flex-1 truncate text-[12.5px] font-medium text-foreground">
              {summaryAgent.name}
            </span>
            <span className={cn(mono, "text-muted-foreground")}>
              {summaryAgent.model}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}

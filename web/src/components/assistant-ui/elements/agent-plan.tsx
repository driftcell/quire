"use client";

import type { ComponentProps } from "react";
import { CheckIcon, MinusIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { mono } from "./surfaces";
import { announced, pct, progressOf } from "../utils/range";

export type AgentPlanStepStatus =
  "pending" | "in_progress" | "completed" | "skipped";

export type AgentPlanStep = {
  id?: string | undefined;
  label: string;
  description?: string | undefined;
  status?: AgentPlanStepStatus | undefined;
};

export function AgentPlan({
  steps,
  activeIndex,
  title,
  hideSingleStepList = false,
  className,
  ...props
}: Omit<ComponentProps<"div">, "children" | "steps" | "activeIndex"> & {
  steps: readonly (string | AgentPlanStep)[];
  activeIndex: number;
  title?: string | undefined;
  hideSingleStepList?: boolean | undefined;
}) {
  const total = steps.length;
  const normalizedSteps: AgentPlanStep[] = steps.map((step) =>
    typeof step === "string" ? { label: step } : step,
  );
  const hasExplicitStatus = normalizedSteps.some((s) => s.status !== undefined);
  const allFinished = activeIndex >= total;

  const completed = allFinished
    ? total
    : hasExplicitStatus
      ? normalizedSteps.filter(
          (s) => s.status === "completed" || s.status === "skipped",
        ).length
      : progressOf(activeIndex, total);

  const allDone = completed >= total;
  const progress = pct(completed, total);

  if (hideSingleStepList && total <= 1) {
    return (
      <div
        data-slot="agent-plan"
        className={cn("sr-only", className)}
        {...props}
      >
        <span>
          {completed} of {total}
        </span>
      </div>
    );
  }

  return (
    <div
      data-slot="agent-plan"
      className={cn("flex w-full flex-col gap-2", className)}
      {...props}
    >
      {title ? (
        <div className="flex items-center justify-between">
          <span className="text-xs font-medium text-muted-foreground">
            {title}
          </span>
          <span className={cn(mono, "text-muted-foreground tabular-nums")}>
            {completed} of {total}
          </span>
        </div>
      ) : (
        <span className="sr-only">
          {completed} of {total}
        </span>
      )}
      <div
        data-slot="agent-plan-progress"
        aria-label={`Plan progress: ${announced(progress)}%`}
        className="h-[2px] w-full overflow-hidden rounded-full bg-foreground/[0.06]"
      >
        <span
          aria-hidden
          className={cn(
            "block h-full rounded-full transition-[width] duration-500 motion-reduce:transition-none",
            allDone ? "bg-emerald-500/80" : "bg-[#1D4ED8]",
          )}
          style={{ width: `${progress}%` }}
        />
      </div>
      <ul className="flex flex-col gap-1.5">
        {normalizedSteps.map((item, i) => {
          const explicit = item.status;
          const skipped = explicit === "skipped";
          const done =
            !skipped &&
            (allDone ||
              (explicit !== undefined
                ? explicit === "completed"
                : i < completed));
          const active =
            !allDone &&
            !skipped &&
            !done &&
            (explicit !== undefined
              ? explicit === "in_progress"
              : i === completed);

          return (
            <li
              key={item.id ?? `${item.label}-${i}`}
              data-step-status={
                skipped
                  ? "skipped"
                  : done
                    ? "completed"
                    : active
                      ? "in_progress"
                      : "pending"
              }
              className={cn(
                "flex gap-2 text-[12.5px] leading-5",
                active && item.description ? "items-start" : "items-center",
              )}
            >
              <span className="flex size-4 shrink-0 items-center justify-center">
                {skipped ? (
                  <MinusIcon
                    aria-hidden
                    className="size-3 text-muted-foreground/50"
                  />
                ) : done ? (
                  <CheckIcon
                    aria-hidden
                    className="size-3.5 text-emerald-500"
                  />
                ) : active ? (
                  <span
                    aria-hidden
                    className="size-2 rounded-full bg-[#1D4ED8] ring-2 ring-[#1D4ED8]/20"
                  />
                ) : (
                  <span
                    aria-hidden
                    className="size-1.5 rounded-full border border-foreground/25"
                  />
                )}
              </span>
              <span className="min-w-0 flex-1">
                <span
                  className={cn(
                    skipped && "text-muted-foreground/50 line-through",
                    done && "text-muted-foreground",
                    active && "font-medium text-foreground",
                    !done && !active && !skipped && "text-muted-foreground/75",
                  )}
                >
                  {item.label}
                </span>
                {active && item.description ? (
                  <span className="mt-0.5 block text-xs text-muted-foreground">
                    {item.description}
                  </span>
                ) : null}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

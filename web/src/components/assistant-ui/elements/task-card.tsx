"use client";

import { Children, type ComponentProps, type ReactNode, useState } from "react";
import { Ban, CheckIcon, ChevronRightIcon, XIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { mono, paper } from "./surfaces";

export type TaskCardState =
  "working" | "waiting" | "done" | "failed" | "cancelled";

const isRenderable = (node: ReactNode) =>
  node !== undefined && node !== null && node !== false && node !== true;

export function TaskStateIcon({
  state,
  className,
}: {
  state: TaskCardState;
  className?: string;
}) {
  if (state === "done") {
    return (
      <CheckIcon
        aria-hidden
        className={cn("size-3.5 shrink-0 text-emerald-500", className)}
      />
    );
  }
  if (state === "failed") {
    return (
      <XIcon
        aria-hidden
        className={cn("size-3.5 shrink-0 text-destructive", className)}
      />
    );
  }
  if (state === "cancelled") {
    return (
      <Ban
        aria-hidden
        className={cn("size-3.5 shrink-0 text-muted-foreground", className)}
      />
    );
  }
  if (state === "working") {
    return (
      <span
        aria-hidden
        className={cn(
          "m-0.5 size-2 shrink-0 rounded-full bg-[#1D4ED8] ring-2 ring-[#1D4ED8]/20",
          className,
        )}
      />
    );
  }
  return (
    <span
      aria-hidden
      className={cn(
        "m-1 size-1.5 shrink-0 rounded-full border border-foreground/35",
        className,
      )}
    />
  );
}

export function TaskCard({
  label,
  meta,
  state,
  elapsed,
  actions,
  result,
  open,
  onOpenChange,
  children,
  className,
  ...props
}: Omit<
  ComponentProps<"div">,
  "children" | "label" | "state" | "result" | "open" | "onOpenChange"
> & {
  label: string;
  meta?: string | undefined;
  state: TaskCardState;
  elapsed?: string | undefined;
  actions?: ReactNode | undefined;
  result?: ReactNode | undefined;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  children?: ReactNode | undefined;
}) {
  const hasTranscript = Children.toArray(children).length > 0;
  const inert = open !== undefined && onOpenChange === undefined;
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const isOpen = open ?? uncontrolledOpen;
  const toggle = () => {
    const next = !isOpen;
    if (open === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  };

  return (
    <div
      data-slot="task-card"
      data-state={state}
      className={cn(
        paper,
        "flex w-full max-w-sm flex-col overflow-hidden rounded-2xl",
        className,
      )}
      {...props}
    >
      <button
        type="button"
        aria-expanded={hasTranscript ? isOpen : undefined}
        disabled={!hasTranscript || inert}
        onClick={toggle}
        className="flex items-center gap-2.5 px-3.5 py-2.5 text-start transition-colors hover:enabled:bg-foreground/[0.03] disabled:cursor-default"
      >
        <TaskStateIcon state={state} />
        <span className="sr-only">{state}</span>
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-foreground">
          {label}
        </span>
        {meta !== undefined && (
          <span
            className={cn(
              mono,
              "max-w-28 shrink-0 truncate text-muted-foreground",
            )}
          >
            {meta}
          </span>
        )}
        {elapsed !== undefined && (
          <span
            className={cn(mono, "shrink-0 text-muted-foreground tabular-nums")}
          >
            {elapsed}
          </span>
        )}
        {hasTranscript && (
          <ChevronRightIcon
            aria-hidden
            className={cn(
              "size-3 shrink-0 text-muted-foreground transition-transform duration-200 motion-reduce:transition-none",
              isOpen && "rotate-90",
            )}
          />
        )}
      </button>
      {isRenderable(actions) && (
        <div
          data-slot="task-card-actions"
          className="border-t border-border/60 px-3.5 py-2.5"
        >
          {actions}
        </div>
      )}
      {hasTranscript && isOpen && (
        <div
          data-slot="task-card-transcript"
          className="flex flex-col gap-2 border-t border-border/60 px-3.5 py-2.5"
        >
          {children}
        </div>
      )}
      {isRenderable(result) && (
        <div
          data-slot="task-card-result"
          className="border-t border-border/60 px-3.5 py-2 text-xs leading-relaxed text-foreground/75"
        >
          {result}
        </div>
      )}
    </div>
  );
}

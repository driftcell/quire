import { memo } from "react";
import {
  AlertCircleIcon,
  CheckIcon,
  ChevronDownIcon,
  CompassIcon,
  Loader2Icon,
} from "lucide-react";
import { cn } from "cn";
import {
  AgentPlan,
  type AgentPlanStep,
} from "@/components/assistant-ui/elements/agent-plan";
import {
  SubagentList,
  type SubagentItem,
} from "@/components/assistant-ui/elements/subagent-list";
import { mono, paper } from "@/components/assistant-ui/elements/surfaces";
import { resolveAgentIdentity } from "@/components/chat/agentAvatar";
import { StopTurnButton } from "@/components/chat/StopTurnButton";
import type { RunStatusModel } from "@/components/chat/traceSegments";

export interface RunActivityHeaderProps {
  model: RunStatusModel;
  isStreaming: boolean;
  detailsOpen: boolean;
  onToggleDetails: () => void;
  onJumpToSection?: (sectionId: string) => void;
  onInterrupt?: () => Promise<void>;
}

/**
 * Vercel / Apple HIG Minimalist Execution Header built on official `assistant-ui Elements`:
 * - Single clean status indicator (no fake pause button, no redundant "主控" pill)
 * - Elapsed duration displayed exactly ONCE
 * - Dynamic Todo-List `AgentPlan` rendered when a multi-step Todo list or multi-stage plan exists
 * - Parallel subagent workstation cards via `SubagentList` with per-subagent Todo Lists & deterministic avatars
 */
export const RunActivityHeader = memo(function RunActivityHeader({
  model,
  isStreaming,
  detailsOpen,
  onToggleDetails,
  onJumpToSection,
  onInterrupt,
}: RunActivityHeaderProps) {
  const completedSubagents = model.subagents.filter(
    (s) => s.status !== "running",
  ).length;
  const subCount = model.subagents.length;
  const isFailed =
    model.status === "failed" || model.status === "interrupted";

  const planSteps: AgentPlanStep[] =
    model.todos.length > 0
      ? model.todos.map((todo, idx) => ({
          id: todo.id ?? `todo-${idx}`,
          label: todo.title,
          description: todo.detail ?? undefined,
          status: todo.status,
        }))
      : model.milestones.length > 0
        ? model.milestones.map((ms) => ({
            id: ms.id,
            label: ms.title,
            description: ms.detail,
            status:
              ms.status === "running"
                ? ("in_progress" as const)
                : ("completed" as const),
          }))
        : [
            {
              id: "default-plan-step",
              label: model.title,
              description: model.detail,
              status: isStreaming
                ? ("in_progress" as const)
                : ("completed" as const),
            },
          ];

  const hasMultiStepPlan =
    model.todos.length > 0 || planSteps.length > 1;

  const activeTodoIndex = planSteps.findIndex(
    (s) => s.status === "in_progress",
  );
  const activePlanIndex = isStreaming
    ? activeTodoIndex >= 0
      ? activeTodoIndex
      : Math.max(
          0,
          planSteps.filter(
            (s) => s.status === "completed" || s.status === "skipped",
          ).length,
        )
    : planSteps.length;

  const subagentItems: SubagentItem[] = model.subagents.map((sub) => {
    const identity = resolveAgentIdentity(sub.actor);
    const subTodos: AgentPlanStep[] = sub.todos.map((t, idx) => ({
      id: t.id ?? `${sub.id}-todo-${idx}`,
      label: t.title,
      description: t.detail ?? undefined,
      status: t.status,
    }));
    const completedTodos = subTodos.filter(
      (t) => t.status === "completed" || t.status === "skipped",
    ).length;

    const progressBadge =
      subTodos.length > 0
        ? `${completedTodos}/${subTodos.length}`
        : sub.toolCount > 0
          ? `${sub.completedToolCount}/${sub.toolCount}`
          : identity.role;

    return {
      id: sub.id,
      actor: sub.actor,
      name: `${identity.label} · ${sub.activityText}`,
      model: progressBadge,
      status: sub.status,
      detail:
        sub.recentTools.length > 0
          ? sub.recentTools.join(" · ")
          : sub.task,
      todos: subTodos.length > 0 ? subTodos : undefined,
    };
  });

  const subagentProgress: number[] = model.subagents.map((sub) => {
    if (sub.todos.length > 0) {
      const doneCount = sub.todos.filter(
        (t) => t.status === "completed" || t.status === "skipped",
      ).length;
      return Math.min(100, Math.round((doneCount / sub.todos.length) * 100));
    }
    if (sub.toolCount > 0) {
      return Math.min(
        100,
        Math.round((sub.completedToolCount / sub.toolCount) * 100),
      );
    }
    return sub.status === "running" ? 45 : 100;
  });

  return (
    <div
      data-live-header="true"
      data-agent-canvas="true"
      data-status={model.status}
      className={cn(
        paper,
        "relative flex w-full flex-col gap-2.5 overflow-hidden rounded-xl px-3.5 py-2.5 text-xs leading-5 transition-colors",
        isStreaming &&
          "sticky top-0 z-10 border-foreground/15 bg-background/95 shadow-2xs backdrop-blur-md",
        isFailed && "border-destructive/35 bg-destructive/5",
      )}
    >
      {/* Top Bar: Clean Status Icon + Title + Single Monospace Telemetry Row */}
      <div className="flex min-h-6 w-full items-center gap-2">
        <button
          type="button"
          data-live-header-toggle="true"
          data-activity-trigger="true"
          aria-expanded={detailsOpen}
          onClick={onToggleDetails}
          className="group/canvas flex min-w-0 flex-1 items-center gap-2 text-left focus-visible:outline-none"
        >
          <span
            data-slot="agent-status"
            className="flex size-4 shrink-0 items-center justify-center"
          >
            {isStreaming ? (
              <Loader2Icon
                aria-hidden
                className="size-3.5 animate-spin text-foreground motion-reduce:animate-none"
              />
            ) : isFailed ? (
              <AlertCircleIcon
                aria-hidden
                className="size-3.5 text-destructive"
              />
            ) : (
              <CheckIcon aria-hidden className="size-3.5 text-emerald-500" />
            )}
          </span>

          <div className="flex min-w-0 flex-1 items-center gap-1.5">
            <span
              className={cn(
                "min-w-0 truncate text-[13px] font-medium text-foreground",
                isStreaming && "shimmer",
              )}
            >
              {model.title}
            </span>
            {model.detail && (
              <span className="hidden min-w-0 truncate text-xs text-muted-foreground sm:inline">
                · {model.detail}
              </span>
            )}
          </div>

          <span
            className={cn(
              mono,
              "ml-auto flex shrink-0 items-center gap-2 text-muted-foreground tabular-nums",
            )}
          >
            {model.toolCount > 0 && (
              <span data-live-progress-tools="true">
                {isStreaming
                  ? `工具 ${model.completedToolCount}/${model.toolCount}`
                  : `${model.toolCount} 次工具`}
              </span>
            )}
            {subCount > 0 && (
              <span data-live-progress-subagents="true">
                {isStreaming
                  ? `子代理 ${completedSubagents}/${subCount}`
                  : `${subCount} 个子代理`}
              </span>
            )}
            {model.elapsedLabel && (
              <span data-live-elapsed="true">{model.elapsedLabel}</span>
            )}
            <ChevronDownIcon
              aria-hidden
              className={cn(
                "size-3.5 shrink-0 text-muted-foreground/70 transition-transform duration-200 group-hover/canvas:text-foreground",
                detailsOpen && "rotate-180",
              )}
            />
          </span>
        </button>

        {model.sectionId && onJumpToSection && (
          <button
            type="button"
            data-live-header-jump="true"
            aria-label="跳转到当前章节"
            title="跳转到当前章节"
            onClick={() => {
              if (model.sectionId) {
                onJumpToSection(model.sectionId);
              }
            }}
            className="inline-flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:text-foreground"
          >
            <CompassIcon className="size-3.5" aria-hidden="true" />
          </button>
        )}

        {isStreaming && onInterrupt && (
          <StopTurnButton onInterrupt={onInterrupt} />
        )}
      </div>

      {/* Dynamic Todo-List AgentPlan: visible only when multi-step todos or stages exist */}
      <div
        className={cn(
          hasMultiStepPlan ? "border-t border-border/50 pt-2.5" : "sr-only",
        )}
      >
        <AgentPlan
          steps={planSteps}
          activeIndex={activePlanIndex}
          hideSingleStepList={!hasMultiStepPlan}
          className="max-w-none"
        />
      </div>

      {/* Parallel Subagent Workstation Roster */}
      {subCount > 0 && (
        <div
          data-slot="subagent-activity-strip"
          className="border-t border-border/50 pt-2.5"
        >
          <SubagentList
            agents={subagentItems}
            completedCount={completedSubagents}
            progress={subagentProgress}
            showSummary={
              isStreaming && subCount > 0 && completedSubagents === subCount
            }
            summaryAgent={{
              name: model.title || "主编汇总成稿",
              model: "汇总中",
            }}
            className={cn(
              "max-w-none",
              subCount > 1 && "grid grid-cols-1 gap-2 sm:grid-cols-2",
            )}
          />
        </div>
      )}
    </div>
  );
});

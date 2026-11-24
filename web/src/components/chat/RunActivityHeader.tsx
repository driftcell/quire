import { memo } from "react";
import { ChevronDownIcon, CompassIcon } from "lucide-react";
import { cn } from "cn";
import {
  AgentPlan,
  type AgentPlanStep,
} from "@/components/assistant-ui/elements/agent-plan";
import {
  AgentStatus,
  type AgentState,
} from "@/components/assistant-ui/elements/agent-status";
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
 * Multi-Agent Orchestration Header built on official `assistant-ui Elements`:
 * - Top status bar with `AgentStatus` pill, telemetry counters, section jump, and stop button
 * - Lead Agent `describe_task` stage plan via `AgentPlan`
 * - Parallel subagent workstation cards via `SubagentList`
 *
 * Maintains an identical DOM layout between live streaming and settled historical
 * states so completing a turn never causes vertical layout shift.
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

  const agentState: AgentState = isStreaming
    ? "working"
    : model.status === "failed" || model.status === "interrupted"
      ? "failed"
      : "done";

  const planSteps: AgentPlanStep[] =
    model.milestones.length > 0
      ? model.milestones.map((ms) => ({
          id: ms.id,
          label: ms.title,
          description: ms.detail,
        }))
      : [
          {
            id: "default-plan-step",
            label: model.title,
            description: model.detail,
          },
        ];

  const runningMilestoneIndex = model.milestones.findIndex(
    (ms) => ms.status === "running",
  );
  const activePlanIndex = isStreaming
    ? runningMilestoneIndex >= 0
      ? runningMilestoneIndex
      : Math.max(0, planSteps.length - 1)
    : planSteps.length;

  const subagentItems: SubagentItem[] = model.subagents.map((sub) => {
    const identity = resolveAgentIdentity(sub.actor);
    return {
      id: sub.id,
      name: `${identity.label} · ${sub.activityText}`,
      model:
        sub.toolCount > 0
          ? `${identity.role} · ${sub.completedToolCount}/${sub.toolCount}`
          : identity.role,
      status: sub.status,
      detail:
        sub.recentTools.length > 0
          ? sub.recentTools.join(" · ")
          : sub.task,
    };
  });

  const subagentProgress: number[] = model.subagents.map((sub) => {
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
        "relative flex w-full flex-col gap-3 overflow-hidden rounded-2xl p-3.5 text-xs leading-5 transition-colors",
        isStreaming &&
          "sticky top-0 z-10 border-foreground/20 bg-background/95 shadow-xs backdrop-blur-md",
        model.status === "failed" && "border-destructive/40 bg-destructive/5",
      )}
    >
      {/* Top Bar: AgentStatus + Current Focus + Telemetry Counters + Expand Toggle */}
      <div className="flex min-h-7 w-full items-center gap-2">
        <button
          type="button"
          data-live-header-toggle="true"
          data-activity-trigger="true"
          aria-expanded={detailsOpen}
          title={
            detailsOpen
              ? "收起 describe_task 阶段里程碑轴"
              : "展开 describe_task 阶段里程碑轴"
          }
          onClick={onToggleDetails}
          className="group/canvas flex min-w-0 flex-1 items-center gap-2.5 text-left focus-visible:outline-none"
        >
          <AgentStatus
            state={agentState}
            label="主控"
            elapsed={model.elapsedLabel}
            className="shrink-0 py-1 ps-2.5 pe-1"
          />

          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <span className="min-w-0 truncate text-[13.5px] font-medium text-foreground">
                {model.title}
              </span>
              {model.detail && (
                <span className="hidden min-w-0 truncate text-xs text-muted-foreground sm:inline">
                  · {model.detail}
                </span>
              )}
            </div>
          </div>

          <span className="ml-auto flex shrink-0 items-center gap-1.5 text-[11px] tabular-nums text-muted-foreground">
            {model.toolCount > 0 && (
              <span
                data-live-progress-tools="true"
                className={cn(
                  mono,
                  "hidden rounded-md bg-foreground/[0.04] px-1.5 py-0.5 sm:inline-flex",
                )}
              >
                {isStreaming
                  ? `工具 ${model.completedToolCount}/${model.toolCount}`
                  : `${model.toolCount} 次工具`}
              </span>
            )}
            {subCount > 0 && (
              <span
                data-live-progress-subagents="true"
                className={cn(
                  mono,
                  "rounded-md bg-foreground/[0.04] px-1.5 py-0.5",
                )}
              >
                {isStreaming
                  ? `子代理 ${completedSubagents}/${subCount}`
                  : `${subCount} 个子代理`}
              </span>
            )}
            {model.elapsedLabel && (
              <span
                data-live-elapsed="true"
                className={cn(
                  mono,
                  "rounded-md bg-foreground/[0.04] px-1.5 py-0.5",
                )}
              >
                {model.elapsedLabel}
              </span>
            )}
            <span className="inline-flex items-center gap-1 rounded-full border border-border/60 bg-foreground/[0.03] px-2.5 py-0.5 text-[11px] font-medium text-foreground/85 transition-colors group-hover/canvas:bg-foreground/[0.07]">
              <span>{detailsOpen ? "收起阶段" : "阶段详情"}</span>
              <ChevronDownIcon
                className={cn(
                  "size-3.5 shrink-0 transition-transform duration-200",
                  detailsOpen && "rotate-180",
                )}
              />
            </span>
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
            className="inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-border/60 bg-background text-muted-foreground transition-colors hover:bg-foreground/[0.05] hover:text-foreground"
          >
            <CompassIcon className="size-3.5" aria-hidden="true" />
          </button>
        )}

        {isStreaming && onInterrupt && (
          <StopTurnButton onInterrupt={onInterrupt} />
        )}
      </div>

      {/* Official assistant-ui AgentPlan driven by describe_task milestones */}
      <div className="border-t border-border/50 pt-2.5">
        <AgentPlan
          title="主控阶段路线"
          steps={planSteps}
          activeIndex={activePlanIndex}
          className="max-w-none"
        />
      </div>

      {/* Official assistant-ui SubagentList for parallel worker roster */}
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
              name: model.title || "主控代理汇总成稿",
              model: "主编统筹",
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

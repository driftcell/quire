import { memo } from "react";
import {
  AlertCircleIcon,
  CheckIcon,
  ChevronDownIcon,
  CompassIcon,
  MinusIcon,
} from "lucide-react";
import { cn } from "cn";
import {
  AgentAvatar,
  resolveAgentIdentity,
} from "@/components/chat/agentAvatar";
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

type LedgerStepStatus = "pending" | "in_progress" | "completed" | "skipped";

interface LedgerStep {
  id: string;
  label: string;
  description?: string;
  status: LedgerStepStatus;
}

function LedgerStepIcon({ status }: { status: LedgerStepStatus }) {
  if (status === "completed") {
    return <CheckIcon aria-hidden className="size-3.5 text-emerald-600" />;
  }
  if (status === "skipped") {
    return (
      <MinusIcon aria-hidden className="size-3 text-muted-foreground/50" />
    );
  }
  if (status === "in_progress") {
    return (
      <span
        aria-hidden
        className="size-1.5 rounded-full bg-[#1D4ED8] ring-2 ring-[#1D4ED8]/15"
      />
    );
  }
  return (
    <span
      aria-hidden
      className="size-1.5 rounded-full border border-foreground/25"
    />
  );
}

function PlanLedger({
  steps,
  expanded,
}: {
  steps: LedgerStep[];
  expanded: boolean;
}) {
  const activeIndex = steps.findIndex((step) => step.status === "in_progress");
  const pendingIndex = steps.findIndex((step) => step.status === "pending");
  const startIndex =
    activeIndex >= 0
      ? activeIndex
      : pendingIndex >= 0
        ? pendingIndex
        : Math.max(0, steps.length - 1);
  const visibleSteps = expanded
    ? steps
    : steps.slice(startIndex, startIndex + 2);

  return (
    <ol
      data-slot="agent-plan"
      data-presentation="ledger"
      aria-label={`执行计划，共 ${steps.length} 步，当前第 ${startIndex + 1} 步`}
      className="mt-3 flex flex-col gap-2"
    >
      {visibleSteps.map((step) => (
        <li
          key={step.id}
          data-step-status={step.status}
          className="flex items-start gap-2.5 text-[12.5px] leading-5"
        >
          <span className="mt-0.5 flex size-4 shrink-0 items-center justify-center">
            <LedgerStepIcon status={step.status} />
          </span>
          <div className="min-w-0 flex-1">
            <span
              className={cn(
                "block break-words",
                step.status === "completed" && "text-muted-foreground",
                step.status === "in_progress" && "font-medium text-foreground",
                step.status === "pending" && "text-muted-foreground/75",
                step.status === "skipped" &&
                  "text-muted-foreground/50 line-through",
              )}
            >
              {step.label}
            </span>
            {step.status === "in_progress" && step.description && (
              <span className="mt-0.5 block text-xs leading-5 text-pretty text-muted-foreground">
                {step.description}
              </span>
            )}
          </div>
        </li>
      ))}
    </ol>
  );
}

function SubagentLedger({
  model,
  expanded,
  onExpand,
}: {
  model: RunStatusModel;
  expanded: boolean;
  onExpand: () => void;
}) {
  const visibleSubagents = expanded
    ? model.subagents
    : model.subagents.slice(0, 4);
  const hiddenCount = model.subagents.length - visibleSubagents.length;

  return (
    <div
      data-slot="subagent-list"
      data-presentation="ledger"
      className="mt-3.5 flex flex-col gap-3"
    >
      {visibleSubagents.map((sub) => {
        const identity = resolveAgentIdentity(sub.actor);
        const statusLabel =
          sub.status === "running"
            ? "进行中"
            : sub.status === "failed"
              ? "未完成"
              : "已完成";

        return (
          <div
            key={sub.id}
            data-subagent-pill={sub.id}
            data-status={sub.status}
            className="flex items-start gap-2.5"
          >
            <AgentAvatar actor={sub.actor} size="xs" className="mt-0.5" />
            <div className="min-w-0 flex-1">
              <div className="flex min-w-0 items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-xs font-medium text-foreground">
                  {identity.label}
                </span>
                <span
                  className={cn(
                    "shrink-0 text-[11px] text-muted-foreground tabular-nums",
                    sub.status === "failed" && "text-destructive",
                  )}
                >
                  {statusLabel}
                </span>
              </div>
              {sub.status === "running" && (
                <p className="mt-0.5 line-clamp-2 text-xs leading-5 text-pretty break-words text-muted-foreground">
                  {sub.activityText}
                </p>
              )}
              {expanded && sub.todos.length > 0 && (
                <ol
                  data-slot="subagent-todo-list"
                  className="mt-2 flex flex-col gap-1.5"
                >
                  {sub.todos.map((todo, index) => (
                    <li
                      key={todo.id ?? `${sub.id}-todo-${index}`}
                      data-step-status={todo.status}
                      className="flex items-start gap-2 text-xs leading-5"
                    >
                      <span className="mt-0.5 flex size-3.5 shrink-0 items-center justify-center">
                        <LedgerStepIcon status={todo.status} />
                      </span>
                      <span
                        className={cn(
                          "min-w-0 break-words",
                          todo.status === "completed" &&
                            "text-muted-foreground",
                          todo.status === "in_progress" &&
                            "font-medium text-foreground",
                          todo.status === "pending" &&
                            "text-muted-foreground/75",
                          todo.status === "skipped" &&
                            "text-muted-foreground/50 line-through",
                        )}
                      >
                        {todo.title}
                      </span>
                    </li>
                  ))}
                </ol>
              )}
            </div>
          </div>
        );
      })}
      {hiddenCount > 0 && (
        <button
          type="button"
          data-slot="subagent-ledger-more"
          onClick={onExpand}
          className="ml-7.5 w-fit rounded-md px-1 py-0.5 text-left text-[11px] text-muted-foreground transition-colors hover:bg-foreground/[0.05] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          另有 {hiddenCount} 位协同角色，展开查看
        </button>
      )}
    </div>
  );
}

function settledTitle(model: RunStatusModel): string {
  if (model.status !== "completed") {
    return model.title;
  }
  if (model.title.startsWith("正在")) {
    return `已完成${model.title.slice(2)}`;
  }
  if (model.title.includes("完成")) {
    return model.title;
  }
  return `已完成${model.title}`;
}

/**
 * Borderless editorial run ledger. Live work stays visible as a calm left-rule
 * checklist; settled runs collapse to a one-line receipt so the answer remains
 * the primary content. It intentionally never uses sticky positioning.
 */
export const RunActivityHeader = memo(function RunActivityHeader({
  model,
  isStreaming,
  detailsOpen,
  onToggleDetails,
  onJumpToSection,
  onInterrupt,
}: RunActivityHeaderProps) {
  const subCount = model.subagents.length;
  const isFailed = model.status === "failed" || model.status === "interrupted";

  const planSteps: LedgerStep[] =
    model.todos.length > 0
      ? model.todos.map((todo, index) => ({
          id: todo.id ?? `todo-${index}`,
          label: todo.title,
          description: todo.detail ?? undefined,
          status: todo.status,
        }))
      : model.milestones
          .filter((milestone) => milestone.isExplicitStage)
          .map((milestone) => ({
            id: milestone.id,
            label: milestone.title,
            description: milestone.detail,
            status:
              milestone.status === "running"
                ? "in_progress"
                : milestone.status === "failed"
                  ? "pending"
                  : "completed",
          }));

  const hasMultiStepPlan = model.todos.length > 0 || planSteps.length > 1;
  const hasExpandableStages = planSteps.length > 0 || subCount > 0;

  return (
    <section
      data-live-header="true"
      data-agent-canvas="true"
      data-status={model.status}
      data-presentation={isStreaming ? "ledger" : "receipt"}
      aria-label={isStreaming ? "当前执行进度" : "执行回执"}
      className={cn(
        "relative w-full border-l-2 py-0.5 pl-3.5 text-xs",
        isStreaming
          ? "border-[#1D4ED8]"
          : isFailed
            ? "border-destructive/60"
            : "border-border/80",
      )}
    >
      <div className="flex min-w-0 items-start gap-1.5">
        <button
          type="button"
          data-live-header-toggle="true"
          data-activity-trigger="true"
          aria-expanded={detailsOpen}
          aria-label={detailsOpen ? "收起执行过程" : "展开执行过程"}
          onClick={onToggleDetails}
          className="group/canvas flex min-w-0 flex-1 items-start gap-2.5 rounded-sm py-0.5 text-left focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <span
            data-slot="agent-status"
            className="mt-1 flex size-4 shrink-0 items-center justify-center"
          >
            {isStreaming ? (
              <span
                aria-hidden
                className="size-2 animate-pulse rounded-full bg-[#1D4ED8] ring-4 ring-[#1D4ED8]/15 motion-reduce:animate-none"
              />
            ) : isFailed ? (
              <AlertCircleIcon
                aria-hidden
                className="size-3.5 text-destructive"
              />
            ) : (
              <CheckIcon aria-hidden className="size-3.5 text-emerald-600" />
            )}
          </span>

          <span className="min-w-0 flex-1">
            <span
              title={model.title}
              className={cn(
                "block break-words text-[13px] font-medium text-foreground",
                isStreaming ? "text-pretty leading-5" : "truncate leading-5.5",
              )}
            >
              {isStreaming ? model.title : settledTitle(model)}
            </span>
          </span>

          <span className="ml-auto flex shrink-0 items-center gap-1.5 pt-0.5 text-[11px] text-muted-foreground tabular-nums">
            {!isStreaming && model.elapsedLabel && (
              <span data-live-elapsed="true">{model.elapsedLabel}</span>
            )}
            {hasExpandableStages && (
              <ChevronDownIcon
                aria-hidden
                className={cn(
                  "size-3.5 text-muted-foreground/60 transition-transform duration-200 group-hover/canvas:text-foreground motion-reduce:transition-none",
                  detailsOpen && "rotate-180",
                )}
              />
            )}
          </span>
        </button>

        {!isStreaming && model.sectionId && onJumpToSection && (
          <button
            type="button"
            data-live-header-jump="true"
            aria-label="定位至对应报告章节"
            title="定位至对应报告章节"
            onClick={() => {
              if (model.sectionId) {
                onJumpToSection(model.sectionId);
              }
            }}
            className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-foreground/[0.05] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            <CompassIcon className="size-3" aria-hidden="true" />
            <span className="hidden @min-[420px]:inline">定位章节</span>
          </button>
        )}
      </div>

      {isStreaming && (
        <div className="ml-6.5">
          {model.detail && (
            <p className="mt-0.5 text-xs leading-5 text-pretty break-words text-muted-foreground">
              {model.detail}
            </p>
          )}
          {(model.elapsedLabel ||
            (model.sectionId && onJumpToSection) ||
            onInterrupt) && (
            <div className="mt-1 flex min-h-6 items-center gap-1">
              {model.elapsedLabel && (
                <span
                  data-live-elapsed="true"
                  className="text-[11px] text-muted-foreground tabular-nums"
                >
                  {model.elapsedLabel}
                </span>
              )}
              <span className="flex-1" />
              {model.sectionId && onJumpToSection && (
                <button
                  type="button"
                  data-live-header-jump="true"
                  aria-label="定位至对应报告章节"
                  title="定位至对应报告章节"
                  onClick={() => {
                    if (model.sectionId) {
                      onJumpToSection(model.sectionId);
                    }
                  }}
                  className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-foreground/[0.05] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                >
                  <CompassIcon className="size-3" aria-hidden="true" />
                  <span className="hidden @min-[420px]:inline">定位章节</span>
                </button>
              )}
              {onInterrupt && <StopTurnButton onInterrupt={onInterrupt} />}
            </div>
          )}
        </div>
      )}

      {((isStreaming && hasMultiStepPlan) ||
        (!isStreaming && detailsOpen && planSteps.length > 0)) && (
        <PlanLedger steps={planSteps} expanded={detailsOpen} />
      )}
      {(isStreaming || detailsOpen) && subCount > 0 && (
        <SubagentLedger
          model={model}
          expanded={detailsOpen}
          onExpand={onToggleDetails}
        />
      )}
    </section>
  );
});

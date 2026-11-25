import { memo, useEffect, useRef } from "react";
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
import type { RunStatusModel } from "@/components/chat/traceSegments";

export interface RunActivityHeaderProps {
  model: RunStatusModel;
  isStreaming: boolean;
  detailsOpen: boolean;
  onToggleDetails: () => void;
  onJumpToSection?: (sectionId: string) => void;
}

type LedgerStepStatus = "pending" | "in_progress" | "completed" | "skipped";

interface LedgerStep {
  id: string;
  label: string;
  description?: string;
  status: LedgerStepStatus;
}

function LedgerStepIcon({
  status,
  muted = false,
}: {
  status: LedgerStepStatus;
  muted?: boolean;
}) {
  if (status === "completed") {
    return (
      <CheckIcon
        aria-hidden
        className={cn(
          "size-3.5",
          muted ? "text-muted-foreground/55" : "text-emerald-600",
        )}
      />
    );
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
  const anchorIndex =
    activeIndex >= 0
      ? activeIndex
      : pendingIndex >= 0
        ? pendingIndex
        : Math.max(0, steps.length - 1);
  const startIndex = expanded
    ? 0
    : Math.max(0, Math.min(anchorIndex - 1, steps.length - 3));
  const endIndex = expanded ? steps.length : startIndex + 3;
  const visibleSteps = steps
    .slice(startIndex, endIndex)
    .map((step, index) => ({ step, index: startIndex + index }));

  return (
    <ol
      data-slot="agent-plan"
      data-presentation={expanded ? "full-todos" : "todo-window"}
      aria-label={`执行计划，共 ${steps.length} 步，当前第 ${anchorIndex + 1} 步`}
      className="mt-3 flex flex-col gap-1.5"
    >
      {visibleSteps.map(({ step, index }) => {
        const isAnchor = !expanded && index === anchorIndex;
        const isSecondary = !expanded && !isAnchor;
        return (
          <li
            key={step.id}
            data-slot="main-todo-item"
            data-step-status={step.status}
            data-focus={isAnchor ? "true" : "false"}
            className={cn(
              "flex items-start gap-2.5 rounded-md px-1.5 py-1 text-[12.5px] leading-5",
              isAnchor && "bg-[#1D4ED8]/[0.045] text-foreground",
              isSecondary && "text-muted-foreground/70",
            )}
          >
            <span className="mt-0.5 flex size-4 shrink-0 items-center justify-center">
              <LedgerStepIcon status={step.status} muted={isSecondary} />
            </span>
            <div className="min-w-0 flex-1">
              <span
                className={cn(
                  "block break-words",
                  step.status === "completed" &&
                    !isAnchor &&
                    "text-muted-foreground/70",
                  step.status === "in_progress" &&
                    "font-medium text-foreground",
                  step.status === "pending" &&
                    !isAnchor &&
                    "text-muted-foreground/65",
                  step.status === "skipped" &&
                    "text-muted-foreground/50 line-through",
                  isAnchor && "font-medium",
                )}
              >
                {step.label}
              </span>
              {(expanded || isAnchor) && step.description && (
                <span className="mt-0.5 block text-xs leading-5 text-pretty text-muted-foreground">
                  {step.description}
                </span>
              )}
            </div>
          </li>
        );
      })}
    </ol>
  );
}

function SubagentAvatarStack({
  model,
  onExpand,
  onFocusSubagent,
}: {
  model: RunStatusModel;
  onExpand: () => void;
  onFocusSubagent: (subagentId: string) => void;
}) {
  const runningSubagents = model.subagents.filter(
    (sub) => sub.status === "running",
  );
  const failedSubagents = model.subagents.filter(
    (sub) => sub.status === "failed",
  );
  const completedSubagents = model.subagents.filter(
    (sub) => sub.status === "completed",
  );
  const remainingSlots = Math.max(0, 6 - runningSubagents.length);
  const visibleSubagents = [
    ...runningSubagents,
    ...[...failedSubagents, ...completedSubagents].slice(0, remainingSlots),
  ];
  const hiddenCount = model.subagents.length - visibleSubagents.length;
  const runningCount = runningSubagents.length;
  const hasFailed = model.subagents.some((sub) => sub.status === "failed");

  return (
    <div
      data-slot="subagent-list"
      data-presentation="avatar-stack"
      aria-label="子代理进度"
      className="mt-3.5 flex items-start gap-2"
    >
      <ol className="flex min-w-0 flex-1 flex-wrap -space-x-1 gap-y-1">
        {visibleSubagents.map((sub) => {
          const identity = resolveAgentIdentity(sub.actor);
          const running = sub.status === "running";
          const failed = sub.status === "failed";
          const statusLabel = running ? "进行中" : failed ? "未完成" : "已完成";
          return (
            <li key={sub.id}>
              <button
                type="button"
                data-subagent-pill={sub.id}
                data-status={sub.status}
                aria-label={`${identity.label}：${statusLabel}${sub.activityText ? `，${sub.activityText}` : ""}。展开并定位`}
                title={`${identity.label} · ${statusLabel}${sub.activityText ? ` · ${sub.activityText}` : ""}`}
                onClick={() => onFocusSubagent(sub.id)}
                className="relative inline-flex size-6 items-center justify-center rounded-full bg-background ring-2 ring-background focus-visible:z-10 focus-visible:ring-[#1D4ED8] focus-visible:outline-none"
              >
                <AgentAvatar
                  actor={sub.actor}
                  size="xs"
                  className={cn(
                    "pointer-events-none",
                    !running && "grayscale opacity-45",
                    running && "ring-2 ring-[#1D4ED8]",
                    failed && "opacity-65",
                  )}
                />
                {running && (
                  <span
                    aria-hidden
                    className="absolute right-0 bottom-0 size-1.5 rounded-full bg-[#1D4ED8] ring-1 ring-background"
                  />
                )}
                {failed && (
                  <span
                    aria-hidden
                    className="absolute right-0 bottom-0 size-1.5 rounded-full bg-destructive ring-1 ring-background"
                  />
                )}
              </button>
            </li>
          );
        })}
        {hiddenCount > 0 && (
          <li>
            <button
              type="button"
              data-slot="subagent-ledger-more"
              aria-label={`另有 ${hiddenCount} 位子代理，展开查看`}
              onClick={onExpand}
              className="inline-flex size-6 items-center justify-center rounded-full bg-muted text-[10px] font-medium text-muted-foreground ring-2 ring-background hover:text-foreground focus-visible:z-10 focus-visible:ring-[#1D4ED8] focus-visible:outline-none"
            >
              +{hiddenCount}
            </button>
          </li>
        )}
      </ol>
      <span
        className={cn(
          "ml-auto shrink-0 pt-1 text-[11px] text-muted-foreground",
          hasFailed && "text-destructive",
        )}
      >
        {hasFailed ? "需关注" : runningCount > 0 ? "协作中" : "已完成"}
      </span>
    </div>
  );
}

function SubagentLedger({
  model,
  expanded,
  onExpand,
  onFocusSubagent,
}: {
  model: RunStatusModel;
  expanded: boolean;
  onExpand: () => void;
  onFocusSubagent: (subagentId: string) => void;
}) {
  if (!expanded) {
    return (
      <SubagentAvatarStack
        model={model}
        onExpand={onExpand}
        onFocusSubagent={onFocusSubagent}
      />
    );
  }

  const visibleSubagents = model.subagents;

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
            data-subagent-id={sub.id}
            data-status={sub.status}
            tabIndex={-1}
            className="flex items-start gap-2.5 rounded-md focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            <AgentAvatar
              actor={sub.actor}
              size="xs"
              className={cn(
                "mt-0.5",
                sub.status !== "running" && "grayscale opacity-55",
                sub.status === "running" && "ring-2 ring-[#1D4ED8]",
              )}
            />
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
              {sub.activityText && (
                <p className="mt-0.5 text-xs leading-5 text-pretty break-words text-muted-foreground">
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
  const activeStepDetail =
    planSteps.find((step) => step.status === "in_progress")?.description ??
    planSteps.find((step) => step.status === "pending")?.description;
  const collapsedDuplicateDetail =
    isStreaming &&
    !detailsOpen &&
    planSteps.length > 0 &&
    Boolean(model.detail) &&
    activeStepDetail === model.detail;
  const rootRef = useRef<HTMLElement>(null);
  const pendingFocusRef = useRef<string | null>(null);

  useEffect(() => {
    const subagentId = pendingFocusRef.current;
    if (!detailsOpen || !subagentId) {
      return;
    }
    pendingFocusRef.current = null;
    const element = rootRef.current?.querySelector(
      `[data-subagent-id="${CSS.escape(subagentId)}"]`,
    );
    if (!(element instanceof HTMLElement)) {
      return;
    }
    const reduceMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    element.focus({ preventScroll: true });
    element.scrollIntoView({
      block: "nearest",
      behavior: reduceMotion ? "auto" : "smooth",
    });
  }, [detailsOpen]);

  const focusSubagent = (subagentId: string) => {
    pendingFocusRef.current = subagentId;
    onToggleDetails();
  };

  return (
    <section
      ref={rootRef}
      data-live-header="true"
      data-agent-canvas="true"
      data-status={model.status}
      data-presentation={isStreaming ? "ledger" : "receipt"}
      aria-label={isStreaming ? "当前执行进度" : "执行回执"}
      className={cn(
        "relative w-full text-xs",
        isStreaming
          ? "rounded-xl border border-border/80 bg-card/70 px-3 py-2.5 shadow-2xs"
          : "py-0.5",
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
            {model.elapsedLabel && (
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
      </div>

      {(isStreaming || detailsOpen) && !collapsedDuplicateDetail && model.detail && (
        <div className="ml-6.5">
          <p className="mt-0.5 text-xs leading-5 text-pretty break-words text-muted-foreground">
            {model.detail}
          </p>
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
          onFocusSubagent={focusSubagent}
        />
      )}
    </section>
  );
});

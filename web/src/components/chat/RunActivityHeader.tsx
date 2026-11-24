import { memo } from "react";
import {
  AlertCircleIcon,
  CheckCircle2Icon,
  ChevronDownIcon,
  CompassIcon,
  NetworkIcon,
  WrenchIcon,
} from "lucide-react";
import { cn } from "cn";
import { AgentAvatar, resolveAgentIdentity } from "@/components/chat/agentAvatar";
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
 * Interactive Multi-Agent Collaboration Canvas Header.
 *
 * Renders the lead orchestrator node, animated SVG dataflow connectors, and
 * satellite subagent cards inside a blueprint dot-grid canvas. Keeps an
 * identical DOM and visual structure between live streaming and settled
 * historical states so the canvas never vanishes or causes layout jitter.
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

  return (
    <div
      data-live-header="true"
      data-agent-canvas="true"
      data-status={model.status}
      className={cn(
        "relative w-full overflow-hidden rounded-2xl border p-3 text-xs leading-5 transition-colors",
        "bg-[radial-gradient(rgba(148,163,184,0.18)_1px,transparent_1px)] [background-size:14px_14px]",
        isStreaming
          ? "sticky top-0 z-10 border-primary/35 bg-background/95 text-foreground shadow-sm backdrop-blur-md"
          : model.status === "failed"
            ? "border-destructive/35 bg-destructive/5 text-foreground"
            : "border-border/70 bg-muted/20 text-muted-foreground",
      )}
    >
      {/* Top Canvas Bar: Lead Orchestrator Node + Telemetry + Controls */}
      <div className="flex min-h-7 w-full items-center gap-2">
        <button
          type="button"
          data-live-header-toggle="true"
          data-activity-trigger="true"
          aria-expanded={detailsOpen}
          title={detailsOpen ? "收起协作轨迹详情" : "展开完整思考与工具轨迹"}
          onClick={onToggleDetails}
          className="group/canvas flex min-w-0 flex-1 items-center gap-2.5 text-left focus-visible:outline-none"
        >
          {/* Orchestrator status orb */}
          <span
            className={cn(
              "inline-flex size-6 shrink-0 items-center justify-center rounded-lg border shadow-2xs transition-colors",
              isStreaming
                ? "border-primary/40 bg-primary/10 text-primary"
                : model.status === "failed" || model.status === "interrupted"
                  ? "border-destructive/30 bg-destructive/10 text-destructive"
                  : "border-emerald-500/30 bg-emerald-500/10 text-emerald-600",
            )}
          >
            {isStreaming ? (
              <NetworkIcon className="size-3.5 animate-pulse" aria-hidden="true" />
            ) : model.status === "failed" || model.status === "interrupted" ? (
              <AlertCircleIcon className="size-3.5" aria-hidden="true" />
            ) : (
              <CheckCircle2Icon className="size-3.5" aria-hidden="true" />
            )}
          </span>

          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <span className="shrink-0 rounded-md border border-border/70 bg-background/90 px-1.5 py-0.2 text-[10px] font-semibold tracking-tight text-foreground/85">
                主控画板
              </span>
              <span
                className={cn(
                  "min-w-0 truncate font-semibold text-foreground",
                  isStreaming && "shimmer",
                )}
              >
                {model.title}
              </span>
              {model.detail && (
                <span className="hidden min-w-0 truncate text-[11px] text-muted-foreground sm:inline">
                  · {model.detail}
                </span>
              )}
            </div>

            {model.recentMainTools.length > 0 && (
              <div className="mt-1 flex flex-wrap items-center gap-1">
                {model.recentMainTools.map((label) => (
                  <span
                    key={label}
                    className="inline-flex items-center gap-1 rounded bg-muted/70 px-1.5 py-0.2 text-[10px] text-muted-foreground"
                  >
                    <WrenchIcon className="size-2.5 opacity-70" aria-hidden="true" />
                    {label}
                  </span>
                ))}
              </div>
            )}
          </div>

          <span className="ml-auto flex shrink-0 items-center gap-2 text-[11px] tabular-nums text-muted-foreground">
            {model.toolCount > 0 && (
              <span
                data-live-progress-tools="true"
                className="hidden rounded-md bg-background/80 px-1.5 py-0.5 ring-1 ring-border/50 sm:inline-flex"
              >
                {isStreaming
                  ? `工具 ${model.completedToolCount}/${model.toolCount}`
                  : `${model.toolCount} 次工具`}
              </span>
            )}
            {subCount > 0 && (
              <span
                data-live-progress-subagents="true"
                className="rounded-md bg-background/80 px-1.5 py-0.5 ring-1 ring-border/50"
              >
                {isStreaming
                  ? `子代理 ${completedSubagents}/${subCount}`
                  : `${subCount} 个子代理`}
              </span>
            )}
            {model.elapsedLabel && (
              <span
                data-live-elapsed="true"
                className="rounded-md bg-background/80 px-1.5 py-0.5 font-medium ring-1 ring-border/50"
              >
                {model.elapsedLabel}
              </span>
            )}
            <span className="inline-flex items-center gap-0.5 rounded-md border border-border/60 bg-background/90 px-2 py-0.5 text-[11px] font-medium text-foreground/80 transition-colors group-hover/canvas:border-border group-hover/canvas:bg-muted/60">
              <span>{detailsOpen ? "收起轨迹" : "展开轨迹"}</span>
              <ChevronDownIcon
                className={cn(
                  "size-3.5 shrink-0 transition-transform duration-150",
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
            className="inline-flex size-6 shrink-0 items-center justify-center rounded-md border border-border/60 bg-background/90 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <CompassIcon className="size-3.5" aria-hidden="true" />
          </button>
        )}

        {isStreaming && onInterrupt && (
          <StopTurnButton onInterrupt={onInterrupt} />
        )}
      </div>

      {/* Subagent Topology Graph Canvas */}
      {subCount > 0 && (
        <div
          data-slot="subagent-activity-strip"
          className="mt-2.5 border-t border-border/40 pt-2"
        >
          {/* SVG branching topology lines connecting Lead Orchestrator -> Subagent Nodes */}
          <svg
            aria-hidden="true"
            viewBox="0 0 240 18"
            preserveAspectRatio="none"
            className="mb-1 h-4 w-full text-border"
          >
            {model.subagents.map((sub, idx) => {
              const targetX =
                subCount === 1
                  ? 120
                  : Math.round(30 + (idx * 180) / Math.max(1, subCount - 1));
              const active = sub.status === "running";
              return (
                <path
                  key={sub.id}
                  d={`M 24 0 C 24 10, ${targetX} 6, ${targetX} 18`}
                  fill="none"
                  stroke="currentColor"
                  strokeWidth={active ? "1.75" : "1.2"}
                  strokeDasharray={active ? "3 2" : undefined}
                  className={cn(
                    active
                      ? "text-primary/70"
                      : sub.status === "failed"
                        ? "text-destructive/50"
                        : "text-border/90",
                  )}
                />
              );
            })}
          </svg>

          {/* Subagent Satellite Node Cards */}
          <div
            className={cn(
              "grid gap-2",
              subCount === 1
                ? "grid-cols-1"
                : subCount === 2
                  ? "grid-cols-1 sm:grid-cols-2"
                  : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-3",
            )}
          >
            {model.subagents.map((sub) => {
              const identity = resolveAgentIdentity(sub.actor);
              const running = sub.status === "running";
              const progressPct =
                sub.toolCount > 0
                  ? Math.min(
                      100,
                      Math.round((sub.completedToolCount / sub.toolCount) * 100),
                    )
                  : running
                    ? 45
                    : 100;

              return (
                <button
                  key={sub.id}
                  type="button"
                  data-subagent-pill={sub.id}
                  data-status={sub.status}
                  onClick={onToggleDetails}
                  title="点击展开或收起子代理详细执行轨迹"
                  className={cn(
                    "group/node flex min-w-0 flex-col gap-1.5 rounded-xl border bg-background/90 p-2.5 text-left shadow-2xs transition-all hover:bg-background",
                    running
                      ? "border-primary/40 ring-1 ring-primary/15"
                      : sub.status === "failed"
                        ? "border-destructive/35 bg-destructive/5"
                        : "border-border/70",
                  )}
                >
                  <div className="flex items-center gap-2">
                    <AgentAvatar
                      actor={sub.actor}
                      size="xs"
                      active={running}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-xs font-semibold text-foreground">
                          {identity.label}
                        </span>
                        <span className="truncate text-[10px] text-muted-foreground">
                          {identity.role}
                        </span>
                      </div>
                    </div>
                    <span
                      className={cn(
                        "inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.2 text-[10px] font-medium tabular-nums",
                        running
                          ? "bg-primary/10 text-primary"
                          : sub.status === "failed"
                            ? "bg-destructive/10 text-destructive"
                            : "bg-emerald-500/10 text-emerald-600",
                      )}
                    >
                      {running && (
                        <span
                          aria-hidden="true"
                          className="size-1.5 animate-pulse rounded-full bg-primary"
                        />
                      )}
                      {sub.toolCount > 0
                        ? `${sub.completedToolCount}/${sub.toolCount}`
                        : running
                          ? "运行中"
                          : sub.status === "failed"
                            ? "失败"
                            : "完成"}
                    </span>
                  </div>

                  {/* Current Live Action / Task Focus */}
                  <div className="flex items-center gap-1.5 text-[11px] text-foreground/85">
                    <span className="truncate font-medium">
                      {sub.activityText}
                    </span>
                  </div>

                  {/* Recent Tool Stream Badges */}
                  {sub.recentTools.length > 0 && (
                    <div className="flex flex-wrap items-center gap-1">
                      {sub.recentTools.map((toolLabel) => (
                        <span
                          key={toolLabel}
                          className="inline-flex items-center rounded bg-muted/75 px-1.5 py-0.2 text-[10px] text-muted-foreground"
                        >
                          {toolLabel}
                        </span>
                      ))}
                    </div>
                  )}

                  {/* Micro Progress Bar */}
                  <div className="h-1 w-full overflow-hidden rounded-full bg-muted/80">
                    <div
                      className={cn(
                        "h-full rounded-full transition-all duration-300",
                        running
                          ? "bg-primary"
                          : sub.status === "failed"
                            ? "bg-destructive"
                            : "bg-emerald-500/80",
                      )}
                      style={{ width: `${progressPct}%` }}
                    />
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
});

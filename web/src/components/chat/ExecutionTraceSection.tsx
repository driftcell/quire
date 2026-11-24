import { memo, useMemo, useState, type ReactNode } from "react";
import {
  AlertCircleIcon,
  CheckIcon,
  CompassIcon,
  Loader2Icon,
  SparklesIcon,
} from "lucide-react";
import { cn } from "cn";
import {
  RunActivity,
  type RunActivityEntry,
  type RunActivityStatus,
} from "@/components/assistant-ui/elements/run-activity";
import { mono, paper } from "@/components/assistant-ui/elements/surfaces";
import {
  TaskCard,
  type TaskCardState,
} from "@/components/assistant-ui/elements/task-card";
import {
  ToolTimeline,
  type TimelineStep,
} from "@/components/assistant-ui/elements/tool-timeline";
import { resolveAgentIdentity } from "@/components/chat/agentAvatar";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import {
  buildHistoricalMilestones,
  buildLiveMilestones,
  extractToolTargetHint,
  type MilestoneSubagentCard,
  type SemanticActionPill,
  type TaskMilestoneNode,
} from "@/components/chat/taskMilestones";
import { getToolMeta } from "@/components/chat/toolMeta";
import {
  buildHistoricalRunStatusModel,
  buildHistoricalSegments,
  hasTraceActivity,
  type RenderSegment,
} from "@/components/chat/traceSegments";
import type { RunTraceSummary } from "@/contracts/session-protocol";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

function collectQuestionNodes(
  segments: RenderSegment[],
  questionSlots?: Map<string, ReactNode>,
): ReactNode[] {
  if (!questionSlots || questionSlots.size === 0) {
    return [];
  }
  const nodes: ReactNode[] = [];
  for (const seg of segments) {
    if (seg.kind !== "activity") continue;
    for (const item of seg.items) {
      if (item.kind !== "execution") continue;
      for (const tool of item.tools) {
        const slot = questionSlots.get(tool.toolCallId);
        if (slot) {
          nodes.push(slot);
        }
      }
    }
  }
  return nodes;
}

function SemanticPillBar({ pills }: { pills: SemanticActionPill[] }) {
  if (pills.length === 0) {
    return null;
  }
  return (
    <div
      data-slot="semantic-action-pills"
      className="flex flex-wrap items-center gap-1.5"
    >
      {pills.map((pill) => (
        <span
          key={pill.key}
          data-pill-status={pill.status}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs transition-colors",
            pill.status === "running"
              ? "border-blue-500/35 bg-blue-500/10 text-foreground"
              : pill.status === "failed"
                ? "border-destructive/35 bg-destructive/10 text-destructive"
                : "border-border/60 bg-foreground/[0.04] text-foreground/85",
          )}
        >
          {pill.status === "running" ? (
            <Loader2Icon
              aria-hidden
              className="size-3 shrink-0 animate-spin text-blue-500"
            />
          ) : pill.status === "failed" ? (
            <AlertCircleIcon
              aria-hidden
              className="size-3 shrink-0 text-destructive"
            />
          ) : (
            <CheckIcon
              aria-hidden
              className="size-3 shrink-0 text-emerald-500"
            />
          )}
          <span className="font-medium">{pill.label}</span>
          {pill.count > 1 && (
            <span className={cn(mono, "text-muted-foreground")}>
              ×{pill.count}
            </span>
          )}
          {pill.targets.length > 0 && (
            <span className={cn(mono, "text-muted-foreground truncate max-w-36")}>
              · {pill.targets.join(", ")}
            </span>
          )}
        </span>
      ))}
    </div>
  );
}

function SubagentTaskCardItem({
  sub,
  onJumpToSection,
}: {
  sub: MilestoneSubagentCard;
  onJumpToSection?: (sectionId: string) => void;
}) {
  const [timelineOpen, setTimelineOpen] = useState(false);
  const identity = resolveAgentIdentity(sub.actor);
  const cardState: TaskCardState =
    sub.status === "running"
      ? "working"
      : sub.status === "failed"
        ? "failed"
        : "done";

  const steps: TimelineStep[] = useMemo(
    () =>
      sub.rawTools.map((tool) => {
        const meta = getToolMeta(tool.toolName);
        return {
          id: tool.toolCallId,
          verb: meta.label,
          chip: extractToolTargetHint(tool) ?? tool.toolName,
          icon: meta.icon,
        };
      }),
    [sub.rawTools],
  );

  const resultContent =
    sub.semanticPills.length > 0 || sub.summaryText ? (
      <div className="flex flex-col gap-2">
        <SemanticPillBar pills={sub.semanticPills} />
        {sub.summaryText && (
          <p className="line-clamp-2 text-xs text-muted-foreground">
            {sub.summaryText}
          </p>
        )}
      </div>
    ) : undefined;

  return (
    <TaskCard
      label={`${identity.label} · ${sub.focusTitle || sub.task}`}
      meta={identity.role}
      state={cardState}
      elapsed={
        sub.toolCount > 0
          ? `${sub.completedToolCount}/${sub.toolCount} 步`
          : undefined
      }
      result={resultContent}
      className="max-w-none"
    >
      <div className="flex flex-col gap-2.5 text-xs">
        <div className="flex items-start justify-between gap-2">
          <p className="text-muted-foreground leading-relaxed">{sub.task}</p>
          {sub.sectionId && onJumpToSection && (
            <button
              type="button"
              onClick={() => {
                if (sub.sectionId) {
                  onJumpToSection(sub.sectionId);
                }
              }}
              className={cn(
                mono,
                "inline-flex shrink-0 items-center gap-1 rounded-md border border-border/60 bg-foreground/[0.04] px-2 py-0.5 text-foreground/80 hover:bg-foreground/[0.08]",
              )}
            >
              <CompassIcon className="size-3" aria-hidden />#{sub.sectionId}
            </button>
          )}
        </div>

        {sub.milestones.length > 0 && (
          <div className="flex flex-col gap-1.5 border-l border-border/60 pl-2.5">
            {sub.milestones.map((ms) => (
              <div key={ms.id} className="flex flex-col gap-1">
                <div className="flex items-center gap-1.5">
                  <span className={cn(mono, "text-muted-foreground")}>
                    0{ms.stepNumber}
                  </span>
                  <span className="font-medium text-foreground/90">
                    {ms.title}
                  </span>
                  {ms.detail && (
                    <span className="text-muted-foreground truncate">
                      · {ms.detail}
                    </span>
                  )}
                </div>
                <SemanticPillBar pills={ms.semanticPills} />
              </div>
            ))}
          </div>
        )}

        {steps.length > 0 && (
          <ToolTimeline
            steps={steps}
            visibleSteps={steps.length}
            streaming={sub.status === "running"}
            open={timelineOpen}
            onOpenChange={setTimelineOpen}
            restingLabel={`查看子代理 ${steps.length} 项工具轨迹`}
            activeLabel={`正在执行子代理操作 (${steps.length})...`}
            stats={[]}
            className="max-w-none"
          />
        )}
      </div>
    </TaskCard>
  );
}

function MilestoneStageCard({
  milestone,
  onJumpToSection,
}: {
  milestone: TaskMilestoneNode;
  onJumpToSection?: (sectionId: string) => void;
}) {
  const [toolTimelineOpen, setToolTimelineOpen] = useState(false);

  const timelineSteps: TimelineStep[] = useMemo(
    () =>
      milestone.rawTools.map((tool) => {
        const meta = getToolMeta(tool.toolName);
        return {
          id: tool.toolCallId,
          verb: meta.label,
          chip: extractToolTargetHint(tool) ?? tool.toolName,
          icon: meta.icon,
        };
      }),
    [milestone.rawTools],
  );

  return (
    <div
      data-slot="describe-task-milestone"
      data-milestone-status={milestone.status}
      className={cn(
        paper,
        "flex flex-col gap-2.5 rounded-2xl p-3.5 transition-colors",
      )}
    >
      {/* Stage Header: Step Number + describe_task Title + Section Jump */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex items-center gap-2">
            <span
              className={cn(
                mono,
                "inline-flex shrink-0 items-center rounded-md bg-foreground/[0.06] px-1.5 py-0.5 text-foreground/80",
              )}
            >
              STAGE {String(milestone.stepNumber).padStart(2, "0")}
            </span>
            <span className="min-w-0 truncate text-[13.5px] font-medium text-foreground">
              {milestone.title}
            </span>
          </div>
          {milestone.detail && (
            <p className="text-muted-foreground text-xs leading-relaxed">
              {milestone.detail}
            </p>
          )}
        </div>

        {milestone.sectionId && onJumpToSection && (
          <button
            type="button"
            onClick={() => {
              if (milestone.sectionId) {
                onJumpToSection(milestone.sectionId);
              }
            }}
            className={cn(
              mono,
              "inline-flex shrink-0 items-center gap-1 rounded-full border border-border/60 bg-foreground/[0.03] px-2.5 py-0.5 text-foreground/80 transition-colors hover:bg-foreground/[0.08]",
            )}
          >
            <CompassIcon className="size-3" aria-hidden />#{milestone.sectionId}
          </button>
        )}
      </div>

      {/* Semantic Aggregated Action Capsules (Default High-Signal View) */}
      <SemanticPillBar pills={milestone.semanticPills} />

      {/* Spawned Subagent Workstation TaskCards in this Milestone */}
      {milestone.subagents.length > 0 && (
        <div className="grid grid-cols-1 gap-2 pt-0.5">
          {milestone.subagents.map((sub) => (
            <SubagentTaskCardItem
              key={sub.id}
              sub={sub}
              onJumpToSection={onJumpToSection}
            />
          ))}
        </div>
      )}

      {/* Official assistant-ui ToolTimeline (Collapsed by Default) */}
      {timelineSteps.length > 0 && (
        <ToolTimeline
          steps={timelineSteps}
          visibleSteps={timelineSteps.length}
          streaming={milestone.status === "running"}
          open={toolTimelineOpen}
          onOpenChange={setToolTimelineOpen}
          restingLabel={`查看本阶段 ${timelineSteps.length} 项工具步序`}
          activeLabel={`正在执行本阶段工具 (${timelineSteps.length})...`}
          stats={[]}
          className="max-w-none"
        />
      )}

      {/* Optional Raw Thinking Disclosure (Hidden by Default per user preference) */}
      {milestone.rawThinking && (
        <details className="group/thinking text-xs text-muted-foreground">
          <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 rounded-md py-0.5 text-xs text-muted-foreground/80 transition-colors hover:text-foreground [&::-webkit-details-marker]:hidden">
            <SparklesIcon className="size-3 shrink-0" aria-hidden />
            <span>查看本阶段思考摘录</span>
          </summary>
          <div className="mt-1.5 rounded-xl border border-border/50 bg-foreground/[0.02] p-2.5 text-xs leading-relaxed whitespace-pre-wrap text-muted-foreground">
            {milestone.rawThinking}
          </div>
        </details>
      )}
    </div>
  );
}

const MilestoneRunActivityPanel = memo(function MilestoneRunActivityPanel({
  milestones,
  status,
  elapsedLabel,
  open,
  onOpenChange,
  onJumpToSection,
}: {
  milestones: TaskMilestoneNode[];
  status: RunActivityStatus;
  elapsedLabel?: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onJumpToSection?: (sectionId: string) => void;
}) {
  const entries: RunActivityEntry[] = useMemo(
    () =>
      milestones.map((ms) => ({
        id: ms.id,
        kind: "tool" as const,
        label: ms.title,
        content: (
          <MilestoneStageCard
            milestone={ms}
            onJumpToSection={onJumpToSection}
          />
        ),
      })),
    [milestones, onJumpToSection],
  );

  if (entries.length === 0) {
    return null;
  }

  return (
    <RunActivity
      status={status}
      statusLabel={`describe_task 阶段里程碑轴 (${entries.length} 个阶段)`}
      durationLabel={elapsedLabel ? `· ${elapsedLabel}` : undefined}
      entries={entries}
      open={open}
      onOpenChange={onOpenChange}
    />
  );
});

export function HistoricalRunTrace({
  trace,
  questionSlots,
  onJumpToSection,
}: {
  sessionId: string;
  trace: RunTraceSummary;
  questionSlots?: Map<string, ReactNode>;
  onJumpToSection?: (sectionId: string) => void;
}) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const segments = useMemo(() => buildHistoricalSegments(trace), [trace]);
  const milestones = useMemo(() => buildHistoricalMilestones(trace), [trace]);
  const showHeader =
    hasTraceActivity(segments) ||
    Boolean(trace.task_focus) ||
    milestones.length > 0;

  if (segments.length === 0 && !showHeader) {
    return null;
  }

  const headerModel = buildHistoricalRunStatusModel(trace);
  const lastTextSegment = [...segments]
    .reverse()
    .find((seg) => seg.kind === "text");
  const compactQuestions = collectQuestionNodes(segments, questionSlots);

  if (!showHeader) {
    return lastTextSegment && lastTextSegment.kind === "text" ? (
      <SmoothStreamdown
        key={lastTextSegment.key}
        text={lastTextSegment.text}
        isStreaming={false}
        className={CHAT_PROSE}
      />
    ) : null;
  }

  const activityStatus: RunActivityStatus =
    headerModel.status === "failed"
      ? "error"
      : headerModel.status === "interrupted"
        ? "cancelled"
        : "complete";

  return (
    <div className="w-full space-y-2.5">
      <RunActivityHeader
        model={headerModel}
        isStreaming={false}
        detailsOpen={detailsOpen}
        onToggleDetails={() => setDetailsOpen((open) => !open)}
        onJumpToSection={onJumpToSection}
      />

      {detailsOpen && (
        <MilestoneRunActivityPanel
          milestones={milestones}
          status={activityStatus}
          elapsedLabel={headerModel.elapsedLabel}
          open={detailsOpen}
          onOpenChange={setDetailsOpen}
          onJumpToSection={onJumpToSection}
        />
      )}

      {compactQuestions.length > 0 && (
        <div className="space-y-2">{compactQuestions}</div>
      )}

      {lastTextSegment && lastTextSegment.kind === "text" && (
        <SmoothStreamdown
          key={lastTextSegment.key}
          text={lastTextSegment.text}
          isStreaming={false}
          className={CHAT_PROSE}
        />
      )}
    </div>
  );
}

export function LiveTraceSegments({
  trace,
  subagents = [],
  segments,
  questionSlots,
  detailsOpen = false,
  onOpenChange,
  onJumpToSection,
}: {
  sessionId: string;
  trace?: LiveAgentTrace;
  subagents?: LiveSubagentRun[];
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  detailsOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  onJumpToSection?: (sectionId: string) => void;
}) {
  const milestones = useMemo(
    () => (trace ? buildLiveMilestones(trace, subagents) : []),
    [trace, subagents],
  );
  const lastTextSegment = [...segments]
    .reverse()
    .find((seg) => seg.kind === "text");
  const compactQuestions = collectQuestionNodes(segments, questionSlots);

  return (
    <>
      {detailsOpen && milestones.length > 0 && (
        <MilestoneRunActivityPanel
          milestones={milestones}
          status={trace?.isStreaming ? "running" : "complete"}
          open={detailsOpen}
          onOpenChange={(next) => onOpenChange?.(next)}
          onJumpToSection={onJumpToSection}
        />
      )}

      {compactQuestions.length > 0 && (
        <div className="space-y-2">{compactQuestions}</div>
      )}

      {lastTextSegment && lastTextSegment.kind === "text" && (
        <SmoothStreamdown
          key={lastTextSegment.key}
          text={lastTextSegment.text}
          isStreaming={lastTextSegment.isStreaming}
          className={CHAT_PROSE}
        />
      )}
    </>
  );
}

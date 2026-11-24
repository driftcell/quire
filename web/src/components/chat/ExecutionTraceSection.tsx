import { memo, useMemo, useState, type ReactNode } from "react";
import {
  AlertCircleIcon,
  CheckIcon,
  CompassIcon,
  Loader2Icon,
  MinusIcon,
  SparklesIcon,
} from "lucide-react";
import { cn } from "cn";
import { mono, paper } from "@/components/assistant-ui/elements/surfaces";
import {
  TaskCard,
  type TaskCardState,
} from "@/components/assistant-ui/elements/task-card";
import { resolveAgentIdentity } from "@/components/chat/agentAvatar";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import {
  buildHistoricalMilestones,
  buildLiveMilestones,
  type MilestoneSubagentCard,
  type SemanticActionPill,
  type TaskMilestoneNode,
} from "@/components/chat/taskMilestones";
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

/**
 * Clean, domain-level semantic action tags (e.g., "查询高教全景数据 · 浙江大学",
 * "登记数据来源", "写入报告内容") without raw `×N` tool repeat multipliers or
 * `#block_id` strings.
 */
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
              ? "border-foreground/25 bg-foreground/[0.06] text-foreground"
              : pill.status === "failed"
                ? "border-destructive/35 bg-destructive/10 text-destructive"
                : "border-border/65 bg-foreground/[0.03] text-foreground/85",
          )}
        >
          {pill.status === "running" ? (
            <Loader2Icon
              aria-hidden
              className="size-3 shrink-0 animate-spin text-foreground"
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
          {pill.targets.length > 0 && (
            <span className="max-w-44 truncate text-muted-foreground">
              · {pill.targets.join("、")}
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
  const identity = resolveAgentIdentity(sub.actor);
  const cardState: TaskCardState =
    sub.status === "running"
      ? "working"
      : sub.status === "failed"
        ? "failed"
        : "done";

  const completedTodos = sub.todos.filter(
    (t) => t.status === "completed" || t.status === "skipped",
  ).length;
  const todoProgressLabel =
    sub.todos.length > 0 ? `${completedTodos}/${sub.todos.length}` : undefined;

  const resultContent =
    sub.todos.length > 0 ||
    sub.semanticPills.length > 0 ||
    sub.summaryText ? (
      <div className="flex flex-col gap-2">
        {sub.todos.length > 0 && (
          <ul className="flex flex-col gap-1">
            {sub.todos.map((todo, idx) => {
              const skipped = todo.status === "skipped";
              const done = !skipped && todo.status === "completed";
              const active =
                !skipped && !done && todo.status === "in_progress";
              return (
                <li
                  key={todo.id ?? `${todo.title}-${idx}`}
                  className="flex items-center gap-1.5 text-xs"
                >
                  {skipped ? (
                    <MinusIcon className="size-3 shrink-0 text-muted-foreground/50" />
                  ) : done ? (
                    <CheckIcon className="size-3 shrink-0 text-emerald-500" />
                  ) : active ? (
                    <Loader2Icon className="size-3 shrink-0 animate-spin text-foreground" />
                  ) : (
                    <span className="mx-0.5 size-1.5 shrink-0 rounded-full border border-foreground/30" />
                  )}
                  <span
                    className={cn(
                      "truncate",
                      skipped && "text-muted-foreground/50 line-through",
                      done && "text-muted-foreground",
                      active && "font-medium text-foreground",
                    )}
                  >
                    {todo.title}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
        <SemanticPillBar pills={sub.semanticPills} />
        {sub.summaryText && (
          <p className="line-clamp-3 text-xs leading-relaxed text-muted-foreground">
            {sub.summaryText}
          </p>
        )}
      </div>
    ) : undefined;

  const hasExpandableDetail =
    Boolean(sub.task) || sub.milestones.length > 1 || Boolean(sub.sectionId);

  return (
    <TaskCard
      label={`${identity.label} · ${sub.focusTitle || sub.task}`}
      meta={identity.role}
      state={cardState}
      elapsed={todoProgressLabel}
      result={resultContent}
      className="max-w-none rounded-xl"
    >
      {hasExpandableDetail ? (
        <div className="flex flex-col gap-2.5 text-xs">
          <div className="flex items-start justify-between gap-2">
            <p className="leading-relaxed text-muted-foreground">{sub.task}</p>
            {sub.sectionId && onJumpToSection && (
              <button
                type="button"
                onClick={() => {
                  if (sub.sectionId) {
                    onJumpToSection(sub.sectionId);
                  }
                }}
                className="inline-flex shrink-0 items-center gap-1 rounded-md border border-border/60 bg-foreground/[0.04] px-2 py-0.5 text-xs text-foreground/80 hover:bg-foreground/[0.08]"
              >
                <CompassIcon className="size-3" aria-hidden />
                <span>定位章节</span>
              </button>
            )}
          </div>

          {sub.milestones.length > 1 && (
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
                      <span className="truncate text-muted-foreground">
                        · {ms.detail}
                      </span>
                    )}
                  </div>
                  <SemanticPillBar pills={ms.semanticPills} />
                </div>
              ))}
            </div>
          )}
        </div>
      ) : null}
    </TaskCard>
  );
}

function MilestoneStageCard({
  milestone,
  showStageIndex,
  onJumpToSection,
}: {
  milestone: TaskMilestoneNode;
  showStageIndex: boolean;
  onJumpToSection?: (sectionId: string) => void;
}) {
  return (
    <div
      data-slot="describe-task-milestone"
      data-milestone-status={milestone.status}
      className={cn(
        paper,
        "flex flex-col gap-2.5 rounded-xl p-3 transition-colors",
      )}
    >
      {showStageIndex && (
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <div className="flex items-center gap-2">
              <span
                className={cn(
                  mono,
                  "inline-flex shrink-0 items-center rounded bg-foreground/[0.06] px-1.5 py-0.5 text-muted-foreground tabular-nums",
                )}
              >
                {String(milestone.stepNumber).padStart(2, "0")}
              </span>
              <span className="min-w-0 truncate text-[13px] font-medium text-foreground">
                {milestone.title}
              </span>
            </div>
            {milestone.detail && (
              <p className="text-xs leading-relaxed text-muted-foreground">
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
              className="inline-flex shrink-0 items-center gap-1 rounded-full border border-border/60 bg-foreground/[0.03] px-2.5 py-0.5 text-xs text-foreground/80 transition-colors hover:bg-foreground/[0.08]"
            >
              <CompassIcon className="size-3" aria-hidden />
              <span>定位章节</span>
            </button>
          )}
        </div>
      )}

      <SemanticPillBar pills={milestone.semanticPills} />

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

      {milestone.rawThinking && (
        <details className="group/thinking text-xs text-muted-foreground">
          <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 rounded-md py-0.5 text-xs text-muted-foreground/80 transition-colors hover:text-foreground [&::-webkit-details-marker]:hidden">
            <SparklesIcon className="size-3 shrink-0" aria-hidden />
            <span>查看分析思路</span>
          </summary>
          <div className="mt-1.5 rounded-lg border border-border/50 bg-foreground/[0.02] p-2.5 text-xs leading-relaxed whitespace-pre-wrap text-muted-foreground">
            {milestone.rawThinking}
          </div>
        </details>
      )}
    </div>
  );
}

const MilestoneExpandedList = memo(function MilestoneExpandedList({
  milestones,
  status,
  onJumpToSection,
}: {
  milestones: TaskMilestoneNode[];
  status: string;
  onJumpToSection?: (sectionId: string) => void;
}) {
  if (milestones.length === 0) {
    return null;
  }

  const showStageIndex = milestones.length > 1;

  return (
    <div
      data-slot="run-activity"
      data-status={status}
      className="flex min-w-0 flex-col gap-2"
    >
      {milestones.map((ms) => (
        <MilestoneStageCard
          key={ms.id}
          milestone={ms}
          showStageIndex={showStageIndex}
          onJumpToSection={onJumpToSection}
        />
      ))}
    </div>
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

  return (
    <div className="w-full space-y-2">
      <RunActivityHeader
        model={headerModel}
        isStreaming={false}
        detailsOpen={detailsOpen}
        onToggleDetails={() => setDetailsOpen((open) => !open)}
        onJumpToSection={onJumpToSection}
      />

      {detailsOpen && (
        <MilestoneExpandedList
          milestones={milestones}
          status={headerModel.status}
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
        <MilestoneExpandedList
          milestones={milestones}
          status={trace?.isStreaming ? "running" : "complete"}
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

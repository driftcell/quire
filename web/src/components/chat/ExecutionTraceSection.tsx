import { Fragment, memo, useState, type ReactNode } from "react";
import { CompassIcon, ListTreeIcon, LoaderCircleIcon } from "lucide-react";
import { HistoricalSubagent } from "@/components/chat/HistoricalSubagent";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import { StopTurnButton } from "@/components/chat/StopTurnButton";
import { SubagentTrace } from "@/components/chat/SubagentTrace";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import {
  activityHasQuestion,
  buildHistoricalSegments,
  extractSubagentLiveView,
  summarizeActivity,
  type ActivityRenderSegment,
  type RenderSegment,
} from "@/components/chat/traceSegments";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";
import type { RunTraceSummary } from "@/contracts/session-protocol";
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

function formatActivityDuration(durationMs?: number): string | null {
  if (durationMs === undefined || durationMs <= 0) {
    return null;
  }
  if (durationMs < 1000) {
    return `${durationMs}ms`;
  }
  const totalSeconds = Math.max(1, Math.round(durationMs / 1000));
  if (totalSeconds < 60) {
    return `${totalSeconds}秒`;
  }
  const mins = Math.floor(totalSeconds / 60);
  const secs = totalSeconds % 60;
  return secs > 0 ? `${mins}分${secs}秒` : `${mins}分`;
}

function ActivitySegment({
  sessionId,
  segment,
  questionSlots,
  taskFocus,
  onJumpToSection,
  onInterrupt,
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
  taskFocus?: LiveAgentTrace["taskFocus"];
  onJumpToSection?: (sectionId: string) => void;
  onInterrupt?: () => void | Promise<void>;
}) {
  const hasQuestion = activityHasQuestion(segment.items, questionSlots);
  const summary = summarizeActivity(segment.items);
  const isActive = segment.isActive;
  const defaultOpen = isActive || hasQuestion || segment.isLive === true;
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const open = userOpen ?? defaultOpen;

  const duration = formatActivityDuration(segment.durationMs);
  const runningParts: string[] = [];
  if (isActive) {
    if (summary.runningTools > 0) {
      runningParts.push(`${summary.runningTools} 个工具运行中`);
    }
    if (summary.runningSubagents > 0) {
      runningParts.push(`${summary.runningSubagents} 个子代理运行中`);
    }
  }
  const completedParts: string[] = [];
  if (!isActive) {
    if (duration) {
      completedParts.push(duration);
    }
    if (summary.subagentCount > 0) {
      completedParts.push(`${summary.subagentCount} 个子代理`);
    }
  }

  const activeTitle = taskFocus?.title ?? "正在处理";
  const title = isActive
    ? activeTitle
    : summary.toolCount > 0
      ? `已完成思考与 ${summary.toolCount} 次操作`
      : summary.subagentCount > 0
        ? `已完成思考与 ${summary.subagentCount} 个子代理`
        : "已完成思考";
  const descriptionParts = isActive ? runningParts : completedParts;
  const sectionId = taskFocus?.sectionId;
  const showJump = isActive && Boolean(sectionId) && Boolean(onJumpToSection);

  return (
    <Trace data-slot="activity" data-active={isActive ? "true" : "false"}>
      <TraceCollapsible open={open} onOpenChange={setUserOpen}>
        {isActive ? (
          <div className="flex w-full min-w-0 items-center gap-1">
            <TraceCollapsibleTrigger
              data-activity-trigger="true"
              className="min-w-0 flex-1"
            >
              <TraceLine
                status="running"
                className="h-6 min-w-0 text-xs leading-5"
              >
                <TraceIcon>
                  <LoaderCircleIcon className="size-3.5 animate-spin" />
                </TraceIcon>
                <TraceTitle>{activeTitle}</TraceTitle>
                {taskFocus?.detail && (
                  <span className="min-w-0 truncate text-[11px] text-muted-foreground/75">
                    — {taskFocus.detail}
                  </span>
                )}
                {descriptionParts.length > 0 && (
                  <TraceDescription className="text-[11px]">
                    {descriptionParts.join(" · ")}
                  </TraceDescription>
                )}
              </TraceLine>
            </TraceCollapsibleTrigger>
            {showJump && (
              <button
                type="button"
                data-activity-jump="true"
                aria-label="跳转到当前章节"
                title="跳转到当前章节"
                onClick={() => {
                  if (sectionId) {
                    onJumpToSection?.(sectionId);
                  }
                }}
                className="inline-flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                <CompassIcon className="size-3.5" aria-hidden="true" />
              </button>
            )}
            {onInterrupt && <StopTurnButton onInterrupt={onInterrupt} />}
          </div>
        ) : (
          <TraceCollapsibleTrigger data-activity-trigger="true">
            <TraceLine className="h-6 text-xs leading-5">
              <TraceIcon>
                <ListTreeIcon className="size-3.5 text-muted-foreground" />
              </TraceIcon>
              <TraceTitle>{title}</TraceTitle>
              {descriptionParts.length > 0 && (
                <TraceDescription className="text-[11px]">
                  {descriptionParts.join(" · ")}
                </TraceDescription>
              )}
            </TraceLine>
          </TraceCollapsibleTrigger>
        )}
        <TraceCollapsibleContent className="mt-1.5 space-y-2 border-l pl-4">
          {segment.items.map((item) => {
            if (item.kind === "thinking") {
              return (
                <ThinkingTrace
                  key={item.key}
                  content={item.text}
                  isStreaming={item.isStreaming}
                  isActive={item.isActive}
                  defaultOpen={false}
                  startedAtMs={item.startedAtMs}
                  durationMs={item.durationMs}
                />
              );
            }
            if (item.kind === "text") {
              return (
                <SmoothStreamdown
                  key={item.key}
                  text={item.text}
                  isStreaming={item.isStreaming}
                  className={`${CHAT_PROSE} text-sm`}
                />
              );
            }
            return (
              <Fragment key={item.key}>
                <ToolCallList
                  sessionId={sessionId}
                  tools={item.tools}
                  questionSlots={questionSlots}
                />
                {item.subagents.map((entry) =>
                  entry.kind === "historical" ? (
                    <HistoricalSubagent
                      key={entry.key}
                      sessionId={sessionId}
                      subagent={entry.subagent}
                    />
                  ) : (
                    <LiveSubagent
                      key={entry.key}
                      sessionId={sessionId}
                      sub={entry.subagent}
                    />
                  ),
                )}
              </Fragment>
            );
          })}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

export const RenderSegments = memo(function RenderSegments({
  sessionId,
  segments,
  questionSlots,
  taskFocus,
  onJumpToSection,
  onInterrupt,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  taskFocus?: LiveAgentTrace["taskFocus"];
  onJumpToSection?: (sectionId: string) => void;
  onInterrupt?: () => void | Promise<void>;
}) {
  return (
    <div className="w-full space-y-1">
      {segments.map((segment) => {
        if (segment.kind === "text") {
          return (
            <SmoothStreamdown
              key={segment.key}
              text={segment.text}
              isStreaming={segment.isStreaming}
              className={`${CHAT_PROSE} pt-1`}
            />
          );
        }
        return (
          <ActivitySegment
            key={segment.key}
            sessionId={sessionId}
            segment={segment}
            questionSlots={questionSlots}
            taskFocus={taskFocus}
            onJumpToSection={onJumpToSection}
            onInterrupt={onInterrupt}
          />
        );
      })}
    </div>
  );
});

export function HistoricalRunTrace({
  sessionId,
  trace,
  questionSlots,
}: {
  sessionId: string;
  trace: RunTraceSummary;
  questionSlots?: Map<string, ReactNode>;
}) {
  const segments = buildHistoricalSegments(trace);
  if (segments.length === 0) {
    return null;
  }
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={segments}
      questionSlots={questionSlots}
    />
  );
}

function LiveSubagent({
  sessionId,
  sub,
}: {
  sessionId: string;
  sub: LiveSubagentRun;
}) {
  const {
    thinkingText,
    isThinkingStreaming,
    thinkingDurationMs,
    thinkingStartedAtMs,
    tools,
    streamingText,
  } = extractSubagentLiveView(sub.trace);
  const outputText =
    sub.summary?.trim() ||
    (sub.status === "running" ? streamingText.trim() : "");

  return (
    <SubagentTrace
      sessionId={sessionId}
      model={{
        id: sub.subagentRunId,
        title: sub.displayName?.trim() || sub.agentName,
        focusTitle: sub.trace.taskFocus?.title,
        status: sub.status,
        thinking: thinkingText,
        thinkingStreaming: isThinkingStreaming,
        thinkingStartedAtMs,
        thinkingDurationMs,
        tools,
        outputText,
        outputStreaming: !sub.summary && sub.status === "running",
      }}
    />
  );
}

export function LiveTraceSegments({
  sessionId,
  segments,
  questionSlots,
  taskFocus,
  onJumpToSection,
  onInterrupt,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  taskFocus?: LiveAgentTrace["taskFocus"];
  onJumpToSection?: (sectionId: string) => void;
  onInterrupt?: () => void | Promise<void>;
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={segments}
      questionSlots={questionSlots}
      taskFocus={taskFocus}
      onJumpToSection={onJumpToSection}
      onInterrupt={onInterrupt}
    />
  );
}

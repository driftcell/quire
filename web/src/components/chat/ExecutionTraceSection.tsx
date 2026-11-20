import { Fragment, memo, useState, type ReactNode } from "react";
import { ListTreeIcon, LoaderCircleIcon } from "lucide-react";
import { Streamdown } from "streamdown";
import { HistoricalSubagent } from "@/components/chat/HistoricalSubagent";
import { SubagentTrace } from "@/components/chat/SubagentTrace";
import {
  CHAT_PROSE,
  STREAMDOWN_CONTROLS,
} from "@/components/chat/chatMarkdown";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import {
  activityHasQuestion,
  buildHistoricalSegments,
  buildLiveSegments,
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
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
}) {
  const hasQuestion = activityHasQuestion(segment.items, questionSlots);
  const summary = summarizeActivity(segment.items);
  const isActive = segment.isActive;
  const defaultOpen = isActive || hasQuestion;
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const open = userOpen ?? defaultOpen;

  const duration = formatActivityDuration(segment.durationMs);
  const descriptionParts: string[] = [];
  if (isActive) {
    if (summary.runningTools > 0) {
      descriptionParts.push(`${summary.runningTools} 个工具运行中`);
    }
  } else {
    if (duration) {
      descriptionParts.push(duration);
    }
    if (summary.subagentCount > 0) {
      descriptionParts.push(`${summary.subagentCount} 个子代理`);
    }
  }

  const title = isActive
    ? summary.runningSubagents > 0
      ? `正在处理 · ${summary.runningSubagents} 个子代理运行中`
      : "正在处理"
    : summary.toolCount > 0
      ? `已完成思考与 ${summary.toolCount} 次操作`
      : summary.subagentCount > 0
        ? `已完成思考与 ${summary.subagentCount} 个子代理`
        : "已完成思考";

  return (
    <Trace data-slot="activity" data-active={isActive ? "true" : "false"}>
      <TraceCollapsible open={open} onOpenChange={setUserOpen}>
        <TraceCollapsibleTrigger data-activity-trigger="true">
          <TraceLine
            status={isActive ? "running" : undefined}
            className="h-6 text-xs leading-5"
          >
            <TraceIcon>
              {isActive ? (
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              ) : (
                <ListTreeIcon className="size-3.5 text-muted-foreground" />
              )}
            </TraceIcon>
            <TraceTitle>{title}</TraceTitle>
            {descriptionParts.length > 0 && (
              <TraceDescription className="text-[11px]">
                {descriptionParts.join(" · ")}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
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
                <div key={item.key} className={`${CHAT_PROSE} text-sm`}>
                  <Streamdown
                    mode={item.isStreaming ? "streaming" : "static"}
                    controls={STREAMDOWN_CONTROLS}
                  >
                    {item.text}
                  </Streamdown>
                </div>
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
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
}) {
  return (
    <div className="w-full space-y-1">
      {segments.map((segment) => {
        if (segment.kind === "text") {
          return (
            <div key={segment.key} className={`${CHAT_PROSE} pt-1`}>
              <Streamdown
                mode={segment.isStreaming ? "streaming" : "static"}
                controls={STREAMDOWN_CONTROLS}
              >
                {segment.text}
              </Streamdown>
            </div>
          );
        }
        return (
          <ActivitySegment
            key={segment.key}
            sessionId={sessionId}
            segment={segment}
            questionSlots={questionSlots}
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
  trace,
  subagents,
  questionSlots,
}: {
  sessionId: string;
  trace: LiveAgentTrace;
  subagents: LiveSubagentRun[];
  questionSlots?: Map<string, ReactNode>;
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={buildLiveSegments(trace, subagents)}
      questionSlots={questionSlots}
    />
  );
}

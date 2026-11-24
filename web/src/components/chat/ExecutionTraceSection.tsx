import { useMemo, useState, type ReactNode } from "react";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import { buildHistoricalMilestones } from "@/components/chat/taskMilestones";
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
 * RunActivityHeader owns the complete main/subagent Todo projection. This
 * section only places decisions and the main agent's reader-facing response.
 */
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
  const lastTextSegment =
    trace.status === "completed"
      ? [...segments].reverse().find((seg) => seg.kind === "text")
      : undefined;
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
    <div className="w-full space-y-3">
      <RunActivityHeader
        model={headerModel}
        isStreaming={false}
        detailsOpen={detailsOpen}
        onToggleDetails={() => setDetailsOpen((open) => !open)}
        onJumpToSection={onJumpToSection}
      />

      {compactQuestions.length > 0 && (
        <div className="space-y-3">{compactQuestions}</div>
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
  segments,
  questionSlots,
  showFinalText = false,
}: {
  sessionId: string;
  trace?: LiveAgentTrace;
  subagents?: LiveSubagentRun[];
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  detailsOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  onJumpToSection?: (sectionId: string) => void;
  showFinalText?: boolean;
}) {
  const lastTextSegment = [...segments]
    .reverse()
    .find((seg) => seg.kind === "text");
  const compactQuestions = collectQuestionNodes(segments, questionSlots);

  return (
    <>
      {compactQuestions.length > 0 && (
        <div className="space-y-3">{compactQuestions}</div>
      )}

      {showFinalText &&
        trace?.isStreaming === false &&
        lastTextSegment &&
        lastTextSegment.kind === "text" && (
          <div data-slot="final-main-output">
            <SmoothStreamdown
              key={lastTextSegment.key}
              text={lastTextSegment.text}
              isStreaming={false}
              revealOnMount={true}
              className={CHAT_PROSE}
            />
          </div>
        )}
    </>
  );
}

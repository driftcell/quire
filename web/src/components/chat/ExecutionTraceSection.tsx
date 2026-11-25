import { useMemo, useState } from "react";
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

/**
 * RunActivityHeader owns the complete main/subagent Todo projection. This
 * section only places the settled run header and the main agent's response.
 */
export function HistoricalRunTrace({
  trace,
  onJumpToSection,
}: {
  sessionId: string;
  trace: RunTraceSummary;
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
  showFinalText = false,
}: {
  sessionId: string;
  trace?: LiveAgentTrace;
  subagents?: LiveSubagentRun[];
  segments: RenderSegment[];
  detailsOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  onJumpToSection?: (sectionId: string) => void;
  showFinalText?: boolean;
}) {
  const lastTextSegment = [...segments]
    .reverse()
    .find((seg) => seg.kind === "text");

  if (
    !(
      showFinalText &&
      trace?.isStreaming === false &&
      lastTextSegment &&
      lastTextSegment.kind === "text"
    )
  ) {
    return null;
  }

  return (
    <div data-slot="final-main-output">
      <SmoothStreamdown
        key={lastTextSegment.key}
        text={lastTextSegment.text}
        isStreaming={false}
        revealOnMount={true}
        className={CHAT_PROSE}
      />
    </div>
  );
}

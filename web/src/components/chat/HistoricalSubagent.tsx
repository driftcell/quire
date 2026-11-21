import { SubagentTrace } from "@/components/chat/SubagentTrace";
import { isVisibleTraceTool } from "@/components/chat/toolMeta";
import { extractSubagentLiveView } from "@/components/chat/traceSegments";
import type { RunTraceSubagent } from "@/contracts/session-protocol";
import { liveSubagentFromSummary } from "@/lib/liveTrace";

export function HistoricalSubagent({
  sessionId,
  subagent,
}: {
  sessionId: string;
  subagent: RunTraceSubagent;
}) {
  const live = liveSubagentFromSummary(subagent, false);
  const view = extractSubagentLiveView(live.trace);
  const title = subagent.display_name?.trim() || subagent.agent_name;
  const tools = view.tools.filter(isVisibleTraceTool);
  const status = live.status;

  return (
    <SubagentTrace
      sessionId={sessionId}
      model={{
        id: subagent.subagent_run_id,
        title,
        focusTitle: subagent.task_focus?.title,
        status,
        thinking: view.thinkingText,
        thinkingStreaming: false,
        thinkingDurationMs: view.thinkingDurationMs,
        tools,
        outputText: subagent.summary?.trim() || view.streamingText.trim() || "",
        outputStreaming: false,
      }}
    />
  );
}

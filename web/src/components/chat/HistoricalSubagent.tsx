import { SubagentTrace } from "@/components/chat/SubagentTrace";
import { isVisibleTraceTool } from "@/components/chat/toolMeta";
import { toToolTraceItems } from "@/components/chat/traceSegments";
import type { RunTraceSubagent } from "@/contracts/session-protocol";

export function HistoricalSubagent({
  sessionId,
  subagent,
}: {
  sessionId: string;
  subagent: RunTraceSubagent;
}) {
  const title = subagent.display_name?.trim() || subagent.agent_name;
  const tools = toToolTraceItems(subagent.tools).filter(isVisibleTraceTool);
  const status =
    subagent.status === "running"
      ? "running"
      : subagent.status === "failed"
        ? "failed"
        : "completed";

  return (
    <SubagentTrace
      sessionId={sessionId}
      model={{
        id: subagent.subagent_run_id,
        title,
        focusTitle: subagent.task_focus?.title,
        status,
        thinking: subagent.thinking,
        thinkingStreaming: false,
        tools,
        outputText: subagent.summary?.trim() ?? "",
        outputStreaming: false,
      }}
    />
  );
}

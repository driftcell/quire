import { memo } from "react";
import { Streamdown } from "streamdown";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";

/**
 * Render subagent output markdown directly without artificial smooth-stream
 * character buffering or per-character fade-in spans. Subagent outputs often
 * contain large audit/benchmark GFM tables; bypassing `useSmoothStreamText`
 * prevents empty table skeletons from appearing while cell characters trickle
 * in and avoids mounting thousands of animation spans when expanding a trace.
 */
export const SubagentOutput = memo(function SubagentOutput({
  text,
  streaming,
}: {
  text: string;
  streaming: boolean;
}) {
  return (
    <div
      data-slot="subagent-output"
      className={`${CHAT_PROSE} text-xs text-muted-foreground`}
    >
      <Streamdown
        mode={streaming ? "streaming" : "static"}
        isAnimating={false}
        animated={false}
        controls={false}
      >
        {text}
      </Streamdown>
    </div>
  );
});

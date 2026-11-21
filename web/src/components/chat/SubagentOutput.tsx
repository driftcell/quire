import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";

export function SubagentOutput({
  text,
  streaming,
}: {
  text: string;
  streaming: boolean;
}) {
  return (
    <div data-slot="subagent-output">
      <SmoothStreamdown
        text={text}
        isStreaming={streaming}
        maxBacklog={160}
        controls={false}
        className={`${CHAT_PROSE} text-xs text-muted-foreground`}
      />
    </div>
  );
}

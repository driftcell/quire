import { Streamdown } from "streamdown";
import {
  CHAT_PROSE,
  STREAMDOWN_CONTROLS,
} from "@/components/chat/chatMarkdown";

export function SubagentOutput({
  text,
  streaming,
}: {
  text: string;
  streaming: boolean;
}) {
  return (
    <div className={`${CHAT_PROSE} text-xs text-muted-foreground`}>
      <Streamdown
        mode={streaming ? "streaming" : "static"}
        controls={STREAMDOWN_CONTROLS}
      >
        {text}
      </Streamdown>
    </div>
  );
}

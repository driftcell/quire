import { Streamdown } from "streamdown";
import {
  CHAT_PROSE,
  STREAMDOWN_ANIMATION,
  STREAMDOWN_CONTROLS,
} from "@/components/chat/chatMarkdown";
import { useSmoothStreamText } from "@/hooks/useSmoothStreamText";

export interface SmoothStreamdownProps {
  text: string;
  isStreaming: boolean;
  className?: string;
}

/**
 * Streamdown wrapper with a stable reading-rate buffer. Static/history text is
 * rendered immediately because the hook seeds the current target.
 */
export function SmoothStreamdown({
  text,
  isStreaming,
  className = CHAT_PROSE,
}: SmoothStreamdownProps) {
  const { text: smoothedText, isSmoothing } = useSmoothStreamText(
    text,
    isStreaming,
  );
  const animating = isStreaming || isSmoothing;
  return (
    <div className={className}>
      <Streamdown
        mode={animating ? "streaming" : "static"}
        isAnimating={animating}
        animated={animating ? STREAMDOWN_ANIMATION : false}
        controls={STREAMDOWN_CONTROLS}
      >
        {smoothedText}
      </Streamdown>
    </div>
  );
}

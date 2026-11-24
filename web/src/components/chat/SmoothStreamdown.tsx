import { useMemo, type ComponentType } from "react";
import {
  Streamdown,
  type AllowedTags,
  type ControlsConfig,
  type ExtraProps,
} from "streamdown";
import { ChatEchartsBlock } from "@/components/chat/ChatEchartsBlock";
import {
  CHAT_PROSE,
  STREAMDOWN_ANIMATION,
  STREAMDOWN_CONTROLS,
} from "@/components/chat/chatMarkdown";
import {
  ChatMessageCitationOrderContext,
  normalizeChatMl,
  stripClobberPrefix,
  useChatMessageCitationOrder,
  useChatRichContext,
} from "@/components/chat/chatRichContext";
import {
  InlineCitation,
  type InlineCitationSource,
} from "@/components/ui/inline-citation";
import {
  Source,
  Sources,
  SourcesContent,
  SourcesTrigger,
} from "@/components/ui/sources";
import { useSmoothStreamText } from "@/hooks/useSmoothStreamText";

export interface SmoothStreamdownProps {
  text: string;
  isStreaming: boolean;
  className?: string;
  maxBacklog?: number;
  controls?: ControlsConfig;
}

const CHATML_ALLOWED_TAGS: AllowedTags = {
  citation: ["id"],
  echarts: ["id"],
};

const ChatMlFallbackRenderer: ComponentType<
  Record<string, unknown> & ExtraProps
> = ({ node, ...props }) => {
  const { sources, charts } = useChatRichContext();
  const citationOrderMap = useChatMessageCitationOrder();
  const tagName = node?.tagName;
  const rawId = typeof props.id === "string" ? stripClobberPrefix(props.id) : "";

  if (!rawId) {
    return null;
  }

  if (tagName === "citation") {
    return (
      <InlineCitation
        sourceId={rawId}
        source={sources.get(rawId)}
        index={citationOrderMap.get(rawId)}
      />
    );
  }

  if (tagName === "echarts") {
    return <ChatEchartsBlock chartId={rawId} documentCharts={charts} />;
  }

  return null;
};

export function ChatSourcesFooter({
  citedSourceIds,
  sourcesMap,
}: {
  citedSourceIds: string[];
  sourcesMap: ReadonlyMap<string, InlineCitationSource>;
}) {
  if (citedSourceIds.length === 0) {
    return null;
  }

  return (
    <Sources defaultOpen={false}>
      <SourcesTrigger count={citedSourceIds.length} />
      <SourcesContent>
        {citedSourceIds.map((sourceId, idx) => {
          const entry = sourcesMap.get(sourceId);
          return (
            <Source
              key={sourceId}
              index={idx + 1}
              href={entry?.url ?? undefined}
              title={entry?.title ?? sourceId}
              publisher={entry?.publisher ?? entry?.author ?? undefined}
              excerpt={entry?.excerpt ?? undefined}
            />
          );
        })}
      </SourcesContent>
    </Sources>
  );
}

/**
 * Streamdown wrapper with a stable reading-rate buffer, short ChatML inline
 * `<citation id="..."/>` badges, inline `<echarts id="..."/>` chart figures,
 * and a collapsible shadcn/ui `<Sources>` drawer at the bottom of the message.
 */
export function SmoothStreamdown({
  text,
  isStreaming,
  className = CHAT_PROSE,
  maxBacklog,
  controls = STREAMDOWN_CONTROLS,
}: SmoothStreamdownProps) {
  const { text: smoothedText, isSmoothing } = useSmoothStreamText(
    text,
    isStreaming,
    maxBacklog,
  );
  const { sources } = useChatRichContext();
  const animating = isStreaming || isSmoothing;

  const { markdown, citedSourceIds } = useMemo(
    () => normalizeChatMl(smoothedText),
    [smoothedText],
  );

  const citationOrderMap = useMemo(() => {
    const map = new Map<string, number>();
    citedSourceIds.forEach((id, index) => {
      map.set(id, index + 1);
    });
    return map;
  }, [citedSourceIds]);

  return (
    <ChatMessageCitationOrderContext.Provider value={citationOrderMap}>
      <div className="space-y-2">
        <div className={className}>
          <Streamdown
            mode={animating ? "streaming" : "static"}
            isAnimating={animating}
            animated={animating ? STREAMDOWN_ANIMATION : false}
            controls={controls}
            allowedTags={CHATML_ALLOWED_TAGS}
            fallbackComponent={ChatMlFallbackRenderer}
          >
            {markdown}
          </Streamdown>
        </div>
        <ChatSourcesFooter
          citedSourceIds={citedSourceIds}
          sourcesMap={sources}
        />
      </div>
    </ChatMessageCitationOrderContext.Provider>
  );
}

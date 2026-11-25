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
  STREAMDOWN_CONTROLS,
} from "@/components/chat/chatMarkdown";
import {
  ChatMessageCitationOrderContext,
  isPlaceholderCitationId,
  normalizeXml,
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
  /** Buffer a settled live response from empty once, then settle into static mode. */
  revealOnMount?: boolean;
  controls?: ControlsConfig;
}

const XML_ALLOWED_TAGS: AllowedTags = {
  citation: ["id"],
  echarts: ["id"],
};

function hasRenderableSourceMetadata(
  source: InlineCitationSource | undefined,
): source is InlineCitationSource {
  if (!source) return false;
  const title = source.title?.trim() ?? "";
  const hasDistinctTitle =
    title.length > 0 && title !== source.id && !isPlaceholderCitationId(title);
  return Boolean(
    hasDistinctTitle ||
    source.url?.trim() ||
    source.publisher?.trim() ||
    source.author?.trim() ||
    source.excerpt?.trim() ||
    source.note?.trim(),
  );
}

const XmlFallbackRenderer: ComponentType<
  Record<string, unknown> & ExtraProps
> = ({ node, ...props }) => {
  const { sources, charts } = useChatRichContext();
  const citationOrderMap = useChatMessageCitationOrder();
  const tagName = node?.tagName;
  const rawId =
    typeof props.id === "string" ? stripClobberPrefix(props.id) : "";

  if (!rawId) {
    return null;
  }

  if (tagName === "citation") {
    if (isPlaceholderCitationId(rawId)) {
      return null;
    }
    const source = sources.get(rawId);
    // When sources are registered in this session, suppress dangling / hallucinated IDs
    // that have no metadata instead of rendering raw internal slugs.
    if (!hasRenderableSourceMetadata(source) && sources.size > 0) {
      return null;
    }
    return (
      <InlineCitation
        sourceId={rawId}
        source={source}
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
  const resolvedEntries = useMemo(() => {
    const items: Array<{ sourceId: string; entry: InlineCitationSource }> = [];
    for (const sourceId of citedSourceIds) {
      if (isPlaceholderCitationId(sourceId)) continue;
      const entry = sourcesMap.get(sourceId);
      if (hasRenderableSourceMetadata(entry)) {
        items.push({ sourceId, entry });
      }
    }
    return items;
  }, [citedSourceIds, sourcesMap]);

  if (resolvedEntries.length === 0) {
    return null;
  }

  return (
    <Sources defaultOpen={false}>
      <SourcesTrigger count={resolvedEntries.length} />
      <SourcesContent>
        {resolvedEntries.map(({ sourceId, entry }, idx) => (
          <Source
            key={sourceId}
            index={idx + 1}
            href={entry.url ?? undefined}
            title={entry.title}
            publisher={entry.publisher ?? entry.author ?? undefined}
            excerpt={entry.excerpt ?? undefined}
          />
        ))}
      </SourcesContent>
    </Sources>
  );
}

/**
 * Streamdown wrapper with a 60fps reading-rate buffer (`useSmoothStreamText`),
 * short XML `<citation id="..."/>` inline badges, `<echarts id="..."/>` figures,
 * and a collapsible `<Sources>` drawer. Disables Streamdown's internal word-span
 * animation (`animated={false}`) so character-level smoothing and inline custom
 * tags never suffer from double-animation DOM remounting.
 */
export function SmoothStreamdown({
  text,
  isStreaming,
  className = CHAT_PROSE,
  maxBacklog,
  revealOnMount = false,
  controls = STREAMDOWN_CONTROLS,
}: SmoothStreamdownProps) {
  const { text: smoothedText, isSmoothing } = useSmoothStreamText(
    text,
    isStreaming,
    maxBacklog,
    revealOnMount,
  );
  const { sources } = useChatRichContext();
  const animating = isStreaming || isSmoothing;

  const { markdown, citedSourceIds } = useMemo(
    () => normalizeXml(smoothedText),
    [smoothedText],
  );

  const validCitedSourceIds = useMemo(
    () =>
      citedSourceIds.filter((id) => {
        if (isPlaceholderCitationId(id)) return false;
        const entry = sources.get(id);
        return hasRenderableSourceMetadata(entry) || sources.size === 0;
      }),
    [citedSourceIds, sources],
  );

  const citationOrderMap = useMemo(() => {
    const map = new Map<string, number>();
    validCitedSourceIds.forEach((id, index) => {
      map.set(id, index + 1);
    });
    return map;
  }, [validCitedSourceIds]);

  return (
    <ChatMessageCitationOrderContext.Provider value={citationOrderMap}>
      <div className="space-y-2">
        <div className={className}>
          <Streamdown
            mode={animating ? "streaming" : "static"}
            isAnimating={animating}
            animated={false}
            controls={controls}
            tableMaxHeight="none"
            allowedTags={XML_ALLOWED_TAGS}
            fallbackComponent={XmlFallbackRenderer}
          >
            {markdown}
          </Streamdown>
        </div>
        <ChatSourcesFooter
          citedSourceIds={validCitedSourceIds}
          sourcesMap={sources}
        />
      </div>
    </ChatMessageCitationOrderContext.Provider>
  );
}

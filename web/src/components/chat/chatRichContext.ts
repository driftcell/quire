import { createContext, useContext } from "react";
import type { ResolvedChatChart } from "@/components/chat/ChatEchartsBlock";
import type { InlineCitationSource } from "@/components/ui/inline-citation";
import type { DocumentResponse } from "@/lib/document";

export interface ChatRichContextValue {
  sources: ReadonlyMap<string, InlineCitationSource>;
  charts: ReadonlyMap<string, ResolvedChatChart>;
}

const EMPTY_RICH_CONTEXT: ChatRichContextValue = {
  sources: new Map(),
  charts: new Map(),
};

export const ChatRichContext =
  createContext<ChatRichContextValue>(EMPTY_RICH_CONTEXT);

export function useChatRichContext(): ChatRichContextValue {
  return useContext(ChatRichContext);
}

const EMPTY_ORDER_MAP: ReadonlyMap<string, number> = new Map();

export const ChatMessageCitationOrderContext =
  createContext<ReadonlyMap<string, number>>(EMPTY_ORDER_MAP);

export function useChatMessageCitationOrder(): ReadonlyMap<string, number> {
  return useContext(ChatMessageCitationOrderContext);
}

export function buildChatRichContext(
  docView: DocumentResponse | null | undefined,
): ChatRichContextValue {
  if (!docView) {
    return EMPTY_RICH_CONTEXT;
  }

  const sources = new Map<string, InlineCitationSource>();
  const rawSources = docView.document.sources ?? {};
  const citationMap = docView.computed.citation_map ?? {};
  let fallbackIndex = 1;

  for (const [id, entry] of Object.entries(rawSources)) {
    const computedIdx = citationMap[id];
    sources.set(id, {
      id,
      index:
        typeof computedIdx === "number" ? computedIdx : fallbackIndex++,
      title: entry.title || id,
      url: entry.url ?? null,
      author: entry.author ?? null,
      publishedAt: entry.published_at ?? null,
      publisher: entry.publisher ?? null,
      excerpt: entry.evidence ?? null,
      note: entry.note ?? null,
    });
  }

  const charts = new Map<string, ResolvedChatChart>();
  const chartTables = docView.computed.chart_tables ?? {};
  const chartReqs = docView.computed.chart_requirements ?? {};

  for (const block of docView.document.blocks) {
    if (block.type === "echarts") {
      charts.set(block.id, {
        chart: block,
        tables: chartTables[block.id],
        requirements: chartReqs[block.id],
        title: typeof block.title === "string" ? block.title : undefined,
        note: typeof block.note === "string" ? block.note : undefined,
        sourceNote:
          typeof block.source_note === "string"
            ? block.source_note
            : undefined,
      });
    }
  }

  return { sources, charts };
}

// Single-pass regex matching all citation and echarts markers in exact appearance order
const COMBINED_CHATML_RE =
  /(<citation\s+id=["']([^"']+)["']\s*(?:\/>|>[\s\S]*?<\/citation>))|(\[\^([a-zA-Z0-9_-]{1,48})\](?!:))|(<(?:echarts|chart)\s+(?:id|block_id|preview_id)=["']([^"']+)["']\s*(?:\/>|>[\s\S]*?<\/(?:echarts|chart)>))/gi;

// Guard incomplete trailing ChatML tags while streaming so partial `<citation id="...` doesn't flicker raw HTML
const INCOMPLETE_TRAILING_CHATML_RE =
  /<\/?(?:citation|echarts|chart)(?:\s+[^>]*)$/i;


export function stripClobberPrefix(rawId: string): string {
  const trimmed = rawId.trim();
  return trimmed.startsWith("user-content-")
    ? trimmed.slice("user-content-".length)
    : trimmed;
}

export interface NormalizedChatMlResult {
  markdown: string;
  citedSourceIds: string[];
  chartIds: string[];
}

/**
 * Normalize short ChatML tags (`<citation id="..."/>`, `<echarts id="..."/>`,
 * and inline `[^source_id]` markers) into clean paired tags for Streamdown
 * rendering and extract the ordered deduplicated list of cited source IDs.
 */
export function normalizeChatMl(rawText: string): NormalizedChatMlResult {
  if (!rawText) {
    return { markdown: "", citedSourceIds: [], chartIds: [] };
  }

  const citedSet = new Set<string>();
  const chartSet = new Set<string>();

  const cleaned = rawText.replace(INCOMPLETE_TRAILING_CHATML_RE, "");
  const markdown = cleaned.replace(
    COMBINED_CHATML_RE,
    (
      _full,
      _citationTag: string | undefined,
      citationId: string | undefined,
      _footnoteTag: string | undefined,
      footnoteId: string | undefined,
      _echartsTag: string | undefined,
      echartsId: string | undefined,
    ) => {
      if (citationId !== undefined) {
        const id = citationId.trim();
        if (id) citedSet.add(id);
        return `<citation id="${id}"></citation>`;
      }
      if (footnoteId !== undefined) {
        const id = footnoteId.trim();
        if (id) citedSet.add(id);
        return `<citation id="${id}"></citation>`;
      }
      if (echartsId !== undefined) {
        const id = echartsId.trim();
        if (id) chartSet.add(id);
        return `\n\n<echarts id="${id}"></echarts>\n\n`;
      }
      return _full;
    },
  );

  return {
    markdown,
    citedSourceIds: Array.from(citedSet),
    chartIds: Array.from(chartSet),
  };
}

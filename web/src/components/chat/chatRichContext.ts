import { createContext, useContext } from "react";
import type { ResolvedChatChart } from "@/components/chat/ChatEchartsBlock";
import type { InlineCitationSource } from "@/components/ui/inline-citation";
import type {
  RunTraceSegment,
  RunTraceSummary,
} from "@/contracts/session-protocol";
import type { DocumentResponse } from "@/lib/document";
import { isRecord } from "@/lib/guards";
import type {
  LiveAgentTrace,
  LiveSubagentRun,
  LiveTraceSegment,
} from "@/lib/liveTrace";

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

function registerSourcesFromRawJson(
  rawJson: string | undefined | null,
  sources: Map<string, InlineCitationSource>,
  nextIndex: () => number,
) {
  if (!rawJson || !rawJson.includes("sources")) {
    return;
  }
  try {
    const parsed: unknown = JSON.parse(rawJson);
    if (!isRecord(parsed) || !Array.isArray(parsed.sources)) {
      return;
    }
    for (const item of parsed.sources) {
      if (!isRecord(item)) continue;
      const rawId =
        typeof item.id === "string"
          ? item.id.trim()
          : typeof item.source_id === "string"
            ? item.source_id.trim()
            : "";
      if (!rawId) continue;
      const existing = sources.get(rawId);
      const title =
        (typeof item.title === "string" && item.title.trim()) ||
        existing?.title ||
        rawId;
      sources.set(rawId, {
        id: rawId,
        index: existing?.index ?? nextIndex(),
        title,
        url:
          (typeof item.url === "string" ? item.url : existing?.url) ?? null,
        author:
          (typeof item.author === "string" ? item.author : existing?.author) ??
          null,
        publishedAt:
          (typeof item.published_at === "string"
            ? item.published_at
            : existing?.publishedAt) ?? null,
        publisher:
          (typeof item.publisher === "string"
            ? item.publisher
            : existing?.publisher) ?? null,
        excerpt:
          (typeof item.evidence === "string"
            ? item.evidence
            : typeof item.excerpt === "string"
              ? item.excerpt
              : existing?.excerpt) ?? null,
        note:
          (typeof item.note === "string" ? item.note : existing?.note) ?? null,
      });
    }
  } catch {
    // Ignore partial or malformed JSON while streaming.
  }
}

function scanHistoricalSegmentsForSources(
  segments: RunTraceSegment[] | undefined,
  sources: Map<string, InlineCitationSource>,
  nextIndex: () => number,
) {
  for (const seg of segments ?? []) {
    if (seg.kind !== "execution") continue;
    for (const tool of seg.tools ?? []) {
      if (tool.tool_name === "upsert_sources") {
        registerSourcesFromRawJson(tool.input_preview, sources, nextIndex);
        registerSourcesFromRawJson(tool.output_preview, sources, nextIndex);
      }
    }
    for (const sub of seg.subagents ?? []) {
      scanHistoricalSegmentsForSources(sub.segments, sources, nextIndex);
    }
  }
}

function scanLiveSegmentsForSources(
  segments: LiveTraceSegment[] | undefined,
  sources: Map<string, InlineCitationSource>,
  nextIndex: () => number,
) {
  for (const seg of segments ?? []) {
    if (seg.kind !== "execution") continue;
    for (const tool of seg.tools) {
      if (tool.toolName === "upsert_sources") {
        registerSourcesFromRawJson(tool.inputPreview, sources, nextIndex);
        registerSourcesFromRawJson(tool.outputPreview, sources, nextIndex);
      }
    }
  }
}

export function buildChatRichContext(
  docView: DocumentResponse | null | undefined,
  traces?: RunTraceSummary[],
  liveMain?: LiveAgentTrace | null,
  liveSubagents?: LiveSubagentRun[],
): ChatRichContextValue {
  const sources = new Map<string, InlineCitationSource>();
  let fallbackIndex = 1;
  const nextIndex = () => fallbackIndex++;

  if (docView) {
    const rawSources = docView.document.sources ?? {};
    const citationMap = docView.computed.citation_map ?? {};

    for (const [id, entry] of Object.entries(rawSources)) {
      const computedIdx = citationMap[id];
      sources.set(id, {
        id,
        index:
          typeof computedIdx === "number" ? computedIdx : nextIndex(),
        title: entry.title || id,
        url: entry.url ?? null,
        author: entry.author ?? null,
        publishedAt: entry.published_at ?? null,
        publisher: entry.publisher ?? null,
        excerpt: entry.evidence ?? null,
        note: entry.note ?? null,
      });
    }
  }

  // Also extract sources registered via `upsert_sources` in pure chat mode
  // (where no report document was created or linked).
  for (const trace of traces ?? []) {
    scanHistoricalSegmentsForSources(trace.segments, sources, nextIndex);
  }
  if (liveMain) {
    scanLiveSegmentsForSources(liveMain.segments, sources, nextIndex);
  }
  for (const sub of liveSubagents ?? []) {
    scanLiveSegmentsForSources(sub.trace.segments, sources, nextIndex);
  }

  const charts = new Map<string, ResolvedChatChart>();
  if (docView) {
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
  }

  return { sources, charts };
}

// Single-pass regex matching all citation and echarts markers in exact appearance order
const COMBINED_CHATML_RE =
  /(<citation\s+id=["']([^"']+)["']\s*(?:\/>|>[\s\S]*?<\/citation>))|(\[\^([a-zA-Z0-9_-]{1,48})\](?!:))|(<(?:echarts|chart)\s+(?:id|block_id|preview_id)=["']([^"']+)["']\s*(?:\/>|>[\s\S]*?<\/(?:echarts|chart)>))/gi;

// Guard any incomplete trailing XML tag (`<c`, `<citation id="...`) or footnote (`[^src...`)
// while streaming character-by-character so partial tags never flicker or break layout.
const INCOMPLETE_TRAILING_CHATML_RE =
  /(?:<\/?(?:citation|echarts|chart)(?:\s+[^>]*)?|<\/?[a-zA-Z]{1,12}|\[\^[a-zA-Z0-9_-]*)$/i;

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
 * Normalize short XML tags (`<citation id="..."/>`, `<echarts id="..."/>`,
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

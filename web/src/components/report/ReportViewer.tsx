import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  MessageSquareIcon,
  MessageSquarePlusIcon,
  SparklesIcon,
} from "lucide-react";
import { CustomHtmlView } from "@/components/report/CustomHtmlView";
import { EchartsCanvas } from "@/components/report/EchartsCanvas";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import { InlineMarkdown } from "@/components/report/InlineMarkdown";
import { TableBlockView } from "@/components/report/TableBlockView";
import {
  A4_CONTENT_WIDTH_PX,
  A4_PAGE_HEIGHT_PX,
  A4_PAGE_WIDTH_PX,
  A4_PADDING_X_PX,
  A4_PADDING_Y_PX,
  paginateBlocks,
  type PageMeasurements,
  type PageSlice,
  type TableMeasurement,
} from "@/components/report/pagination";
import type {
  CommentThread,
  ComputedDocumentMeta,
  EphemeralPreviewEvent,
  TextQuoteSelector,
} from "@/contracts/report-protocol";
import type { ReportBlock, ReportDocumentView } from "@/lib/document";
import type { ReportViewMode } from "@/lib/preferences";

export interface ReportViewerProps {
  document: ReportDocumentView;
  computed: ComputedDocumentMeta;
  viewMode: ReportViewMode;
  ephemeralPreviews?: Map<string, EphemeralPreviewEvent>;
  comments?: CommentThread[];
  printMode?: boolean;
  previewMode?: boolean;
  pageFilter?: number;
  onPagesReady?: (totalPages: number) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onAskAgentWithQuote?: (blockId: string, exact: string) => void;
}

interface FloatingQuoteAction {
  blockId: string;
  quote: TextQuoteSelector;
  top: number;
  left: number;
  viewportRect: FloatingCommentAnchor["rect"];
}

function toViewportRect(domRect: DOMRect): FloatingCommentAnchor["rect"] {
  return {
    top: domRect.top,
    left: domRect.left,
    bottom: domRect.bottom,
    right: domRect.right,
    width: domRect.width,
    height: domRect.height,
  };
}

function renderHeadingTag(
  level: number,
  blockId: string,
  text: string,
  citationMap: ComputedDocumentMeta["citation_map"],
  onCitationClick: (sourceId: string) => void,
  highlightQuotes?: TextQuoteSelector[],
  onQuoteClick?: (quote: TextQuoteSelector, rect: DOMRect) => void,
) {
  const content = (
    <InlineMarkdown
      text={text}
      citationMap={citationMap}
      onCitationClick={onCitationClick}
      highlightQuotes={highlightQuotes}
      onQuoteClick={onQuoteClick}
    />
  );
  switch (level) {
    case 1:
      return (
        <h1
          id={`block-${blockId}`}
          className="mt-8 mb-4 text-2xl font-bold tracking-tight"
        >
          {content}
        </h1>
      );
    case 2:
      return (
        <h2
          id={`block-${blockId}`}
          className="mt-7 mb-3 border-b border-black/10 pb-1.5 text-xl font-semibold dark:border-white/10"
        >
          {content}
        </h2>
      );
    case 3:
      return (
        <h3 id={`block-${blockId}`} className="mt-5 mb-2 text-lg font-semibold">
          {content}
        </h3>
      );
    case 4:
      return (
        <h4
          id={`block-${blockId}`}
          className="mt-4 mb-1.5 text-base font-semibold"
        >
          {content}
        </h4>
      );
    default:
      return (
        <h5 id={`block-${blockId}`} className="mt-3 mb-1 text-sm font-semibold">
          {content}
        </h5>
      );
  }
}

function headingPreviewClass(level: number | null | undefined): string {
  switch (level) {
    case 1:
      return "text-2xl font-bold tracking-tight";
    case 2:
      return "text-xl font-semibold";
    case 3:
      return "text-lg font-semibold";
    case 4:
      return "text-base font-semibold";
    default:
      return "text-sm font-semibold";
  }
}

function EphemeralPreviewCard({ preview }: { preview: EphemeralPreviewEvent }) {
  const { block } = preview;
  const cursor = (
    <span className="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-primary align-middle" />
  );

  let body: React.ReactNode;
  if (block.block_type === "heading") {
    body = (
      <div className={headingPreviewClass(block.level)}>
        {block.text || "正在生成标题..."}
        {cursor}
      </div>
    );
  } else if (block.block_type === "quote") {
    body = (
      <blockquote className="border-l-4 border-primary/40 pl-3 text-[15px] leading-7 italic text-foreground/90">
        {block.text || ""}
        {cursor}
      </blockquote>
    );
  } else if (block.block_type === "table") {
    body = block.text ? (
      <p className="m-0 leading-relaxed whitespace-pre-wrap text-foreground/90">
        {block.text}
        {cursor}
      </p>
    ) : (
      <div className="space-y-1.5 rounded-lg border border-black/10 p-2 dark:border-white/10">
        <div className="grid grid-cols-3 gap-2">
          <div className="h-4 animate-pulse rounded bg-muted" />
          <div className="h-4 animate-pulse rounded bg-muted" />
          <div className="h-4 animate-pulse rounded bg-muted" />
        </div>
        <div className="grid grid-cols-3 gap-2">
          <div className="h-3.5 animate-pulse rounded bg-muted/70" />
          <div className="h-3.5 animate-pulse rounded bg-muted/70" />
          <div className="h-3.5 animate-pulse rounded bg-muted/70" />
        </div>
        <div className="grid grid-cols-3 gap-2">
          <div className="h-3.5 animate-pulse rounded bg-muted/50" />
          <div className="h-3.5 animate-pulse rounded bg-muted/50" />
          <div className="h-3.5 animate-pulse rounded bg-muted/50" />
        </div>
      </div>
    );
  } else if (block.block_type === "echarts") {
    body = (
      <div className="flex h-44 items-end justify-around gap-3 rounded-lg bg-muted/40 p-4">
        <div className="h-1/2 w-8 animate-pulse rounded-t bg-primary/20" />
        <div className="h-4/5 w-8 animate-pulse rounded-t bg-primary/25" />
        <div className="h-2/3 w-8 animate-pulse rounded-t bg-primary/20" />
        <div className="h-full w-8 animate-pulse rounded-t bg-primary/30" />
        <div className="h-3/5 w-8 animate-pulse rounded-t bg-primary/20" />
      </div>
    );
  } else if (block.text) {
    body = (
      <p className="m-0 text-[15px] leading-7 whitespace-pre-wrap text-foreground/90">
        {block.text}
        {cursor}
      </p>
    );
  } else {
    body = <div className="h-20 animate-pulse rounded-lg bg-muted/60" />;
  }

  return (
    <div
      data-preview-id={preview.preview_id}
      data-block-id={block.block_id ?? undefined}
      className="my-3 rounded-xl border border-dashed border-primary/40 bg-primary/[0.03] px-4 py-3 text-sm"
    >
      <div className="mb-1.5 flex items-center justify-between text-[11px] text-primary">
        <span className="font-medium">
          {preview.actor} 正在实时生成 ({block.block_type})...
        </span>
        <span className="inline-block size-2 animate-pulse rounded-full bg-primary" />
      </div>
      {block.title && (
        <div className="mb-1.5 font-medium text-foreground">{block.title}</div>
      )}
      {body}
    </div>
  );
}

interface MixedRenderItem {
  key: string;
  slice?: PageSlice;
  preview?: EphemeralPreviewEvent;
}

function buildMixedItems(
  slices: PageSlice[],
  previews: EphemeralPreviewEvent[],
  allBlocks: ReportBlock[],
): MixedRenderItem[] {
  if (previews.length === 0) {
    return slices.map((slice, idx) => ({
      key: `${slice.block.id}-${slice.rowStart ?? 0}-${idx}`,
      slice,
    }));
  }

  const sectionLastBlock = new Map<string, string>();
  let activeSection: string | null = null;
  for (const b of allBlocks) {
    if (b.type === "heading") {
      activeSection = b.id;
    }
    if (activeSection) {
      sectionLastBlock.set(activeSection, b.id);
    }
  }

  const afterMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforeMap = new Map<string, EphemeralPreviewEvent[]>();
  const replaceMap = new Map<string, EphemeralPreviewEvent>();
  const tailPreviews: EphemeralPreviewEvent[] = [];

  for (const p of previews) {
    const anchor = p.anchor;
    if (anchor.type === "replace_block") {
      replaceMap.set(anchor.block_id, p);
    } else if (anchor.type === "before_block") {
      const list = beforeMap.get(anchor.block_id) ?? [];
      list.push(p);
      beforeMap.set(anchor.block_id, list);
    } else if (anchor.type === "after_block") {
      const list = afterMap.get(anchor.block_id) ?? [];
      list.push(p);
      afterMap.set(anchor.block_id, list);
    } else if (anchor.type === "append_to_section") {
      const targetId =
        sectionLastBlock.get(anchor.section_id) ?? anchor.section_id;
      const list = afterMap.get(targetId) ?? [];
      list.push(p);
      afterMap.set(targetId, list);
    } else {
      tailPreviews.push(p);
    }
  }

  const out: MixedRenderItem[] = [];
  for (let idx = 0; idx < slices.length; idx += 1) {
    const slice = slices[idx];
    if (!slice) continue;
    const bid = slice.block.id;

    for (const p of beforeMap.get(bid) ?? []) {
      out.push({ key: `pre-${p.preview_id}`, preview: p });
    }
    const replacement = replaceMap.get(bid);
    if (replacement) {
      out.push({ key: `rep-${replacement.preview_id}`, preview: replacement });
    } else {
      out.push({
        key: `${bid}-${slice.rowStart ?? 0}-${idx}`,
        slice,
      });
    }
    for (const p of afterMap.get(bid) ?? []) {
      out.push({ key: `post-${p.preview_id}`, preview: p });
    }
  }
  for (const p of tailPreviews) {
    out.push({ key: `tail-${p.preview_id}`, preview: p });
  }
  return out;
}

function mixPreviewsIntoPages(
  pages: {
    pageNumber: number;
    items: PageSlice[];
    includeCover?: boolean;
    includeBibliography?: boolean;
  }[],
  previews: EphemeralPreviewEvent[],
  allBlocks: ReportBlock[],
): MixedRenderItem[][] {
  if (previews.length === 0) {
    return pages.map((page) =>
      page.items.map((slice, idx) => ({
        key: `${slice.block.id}-${slice.rowStart ?? 0}-${idx}`,
        slice,
      })),
    );
  }

  const sectionLastBlock = new Map<string, string>();
  let activeSection: string | null = null;
  for (const b of allBlocks) {
    if (b.type === "heading") {
      activeSection = b.id;
    }
    if (activeSection) {
      sectionLastBlock.set(activeSection, b.id);
    }
  }

  const lastOccurByBlock = new Map<
    string,
    { pageIdx: number; sliceIdx: number }
  >();
  for (let pIdx = 0; pIdx < pages.length; pIdx += 1) {
    const page = pages[pIdx];
    if (!page) continue;
    for (let sIdx = 0; sIdx < page.items.length; sIdx += 1) {
      const slice = page.items[sIdx];
      if (slice) {
        lastOccurByBlock.set(slice.block.id, { pageIdx: pIdx, sliceIdx: sIdx });
      }
    }
  }

  const afterMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforeMap = new Map<string, EphemeralPreviewEvent[]>();
  const replaceMap = new Map<string, EphemeralPreviewEvent>();
  const tailPreviews: EphemeralPreviewEvent[] = [];

  for (const p of previews) {
    const anchor = p.anchor;
    if (
      anchor.type === "replace_block" &&
      lastOccurByBlock.has(anchor.block_id)
    ) {
      replaceMap.set(anchor.block_id, p);
    } else if (
      anchor.type === "before_block" &&
      lastOccurByBlock.has(anchor.block_id)
    ) {
      const list = beforeMap.get(anchor.block_id) ?? [];
      list.push(p);
      beforeMap.set(anchor.block_id, list);
    } else if (
      anchor.type === "after_block" &&
      lastOccurByBlock.has(anchor.block_id)
    ) {
      const list = afterMap.get(anchor.block_id) ?? [];
      list.push(p);
      afterMap.set(anchor.block_id, list);
    } else if (anchor.type === "append_to_section") {
      const targetId =
        sectionLastBlock.get(anchor.section_id) ?? anchor.section_id;
      if (lastOccurByBlock.has(targetId)) {
        const list = afterMap.get(targetId) ?? [];
        list.push(p);
        afterMap.set(targetId, list);
      } else {
        tailPreviews.push(p);
      }
    } else {
      tailPreviews.push(p);
    }
  }

  const seenBefore = new Set<string>();
  const seenReplace = new Set<string>();

  return pages.map((page, pIdx) => {
    const out: MixedRenderItem[] = [];
    for (let sIdx = 0; sIdx < page.items.length; sIdx += 1) {
      const slice = page.items[sIdx];
      if (!slice) continue;
      const bid = slice.block.id;

      if (!seenBefore.has(bid)) {
        seenBefore.add(bid);
        for (const p of beforeMap.get(bid) ?? []) {
          out.push({ key: `pre-${p.preview_id}`, preview: p });
        }
      }

      const replacement = replaceMap.get(bid);
      if (replacement) {
        if (!seenReplace.has(bid)) {
          seenReplace.add(bid);
          out.push({
            key: `rep-${replacement.preview_id}`,
            preview: replacement,
          });
        }
      } else {
        out.push({
          key: `${bid}-${slice.rowStart ?? 0}-${sIdx}`,
          slice,
        });
      }

      const lastOccur = lastOccurByBlock.get(bid);
      if (
        lastOccur &&
        lastOccur.pageIdx === pIdx &&
        lastOccur.sliceIdx === sIdx
      ) {
        for (const p of afterMap.get(bid) ?? []) {
          out.push({ key: `post-${p.preview_id}`, preview: p });
        }
      }
    }

    if (pIdx === pages.length - 1) {
      for (const p of tailPreviews) {
        out.push({ key: `tail-${p.preview_id}`, preview: p });
      }
    }
    return out;
  });
}

function blockTitle(block: ReportBlock): string | undefined {
  const title = block.title;
  return typeof title === "string" ? title : undefined;
}

function findBlockElement(node: Node | null): HTMLElement | null {
  let curr: Node | null = node;
  while (curr) {
    if (curr instanceof HTMLElement && curr.dataset.blockId) {
      return curr;
    }
    curr = curr.parentNode;
  }
  return null;
}

interface ReportBlockItemProps {
  slice: PageSlice;
  isPaged: boolean;
  printMode: boolean;
  computed: ComputedDocumentMeta;
  commentCount: number;
  blockQuotes?: TextQuoteSelector[];
  onCitationClick: (sourceId: string) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
}

const ReportBlockItem = memo(function ReportBlockItem({
  slice,
  isPaged,
  printMode,
  computed,
  commentCount,
  blockQuotes,
  onCitationClick,
  onOpenCommentForBlock,
}: ReportBlockItemProps) {
  const { block } = slice;
  const framing = computed.block_framing[block.id];

  const handleQuoteClick = useCallback(
    (quote: TextQuoteSelector, rect: DOMRect) => {
      if (!onOpenCommentForBlock) return;
      onOpenCommentForBlock({
        blockId: block.id,
        quote,
        rect: toViewportRect(rect),
      });
    },
    [block.id, onOpenCommentForBlock],
  );
  const quoteHandler =
    !printMode && onOpenCommentForBlock ? handleQuoteClick : undefined;

  let bodyNode: React.ReactNode = null;
  if (block.type === "heading") {
    bodyNode = renderHeadingTag(
      block.level ?? 2,
      block.id,
      block.text,
      computed.citation_map,
      onCitationClick,
      blockQuotes,
      quoteHandler,
    );
  } else if (block.type === "paragraph") {
    bodyNode = (
      <p
        id={`block-${block.id}`}
        className="my-3 text-[15px] leading-7 text-current/90"
      >
        <InlineMarkdown
          text={block.text}
          citationMap={computed.citation_map}
          onCitationClick={onCitationClick}
          highlightQuotes={blockQuotes}
          onQuoteClick={quoteHandler}
        />
      </p>
    );
  } else if (block.type === "quote") {
    bodyNode = (
      <blockquote
        id={`block-${block.id}`}
        className="my-4 border-l-4 border-current/25 pl-4 text-[15px] leading-7 italic opacity-90"
      >
        <InlineMarkdown
          text={block.text}
          citationMap={computed.citation_map}
          onCitationClick={onCitationClick}
          highlightQuotes={blockQuotes}
          onQuoteClick={quoteHandler}
        />
      </blockquote>
    );
  } else if (block.type === "table") {
    const titleText = block.title?.trim();
    const totalRows =
      slice.totalRows ?? (Array.isArray(block.rows) ? block.rows.length : 0);
    const hasMoreRows =
      slice.rowEnd !== undefined && totalRows > 0 && slice.rowEnd < totalRows;
    const showCaption = Boolean(framing || titleText || slice.continuation);

    bodyNode = (
      <figure id={`block-${block.id}`} className="my-5 space-y-2">
        {showCaption && (
          <figcaption className="flex items-baseline justify-between gap-2 text-sm font-semibold">
            <span>
              {framing ? framing.label : "表"}
              {titleText ? ` — ${titleText}` : ""}
              {slice.continuation ? "（续）" : ""}
            </span>
            {slice.continuation &&
              slice.rowStart !== undefined &&
              slice.rowEnd !== undefined && (
                <span className="text-[11px] font-normal text-muted-foreground tabular-nums">
                  接上页 · 第 {slice.rowStart + 1}–{slice.rowEnd} 行 / 共{" "}
                  {totalRows} 行
                </span>
              )}
          </figcaption>
        )}
        <TableBlockView
          table={block}
          layout={computed.table_layouts[block.id]}
          rowStart={slice.rowStart}
          rowEnd={slice.rowEnd}
          includeSummary={slice.includeSummary}
          containerWidthPx={isPaged ? A4_CONTENT_WIDTH_PX : 700}
        />
        {hasMoreRows && slice.rowEnd !== undefined && (
          <div className="flex items-center justify-end border-t border-dashed border-black/15 pt-1 text-[11px] text-muted-foreground tabular-nums dark:border-white/15">
            （接下页 · 续第 {slice.rowEnd + 1} 行）
          </div>
        )}
        {framing?.attribution && slice.includeSummary !== false && (
          <div className="text-xs text-muted-foreground">
            {framing.attribution}
          </div>
        )}
      </figure>
    );
  } else if (block.type === "echarts") {
    const titleText = blockTitle(block);
    bodyNode = (
      <figure id={`block-${block.id}`} className="my-5 space-y-2">
        {(framing || titleText) && (
          <figcaption className="text-sm font-semibold">
            {framing ? framing.label : "图"}
            {titleText ? ` — ${titleText}` : ""}
          </figcaption>
        )}
        <EchartsCanvas
          blockId={block.id}
          chart={block}
          chartTables={computed.chart_tables?.[block.id]}
          requirements={computed.chart_requirements?.[block.id]}
          renderer={isPaged || printMode ? "svg" : "canvas"}
          printSafe={isPaged || printMode}
        />
        {framing?.attribution && (
          <div className="text-xs text-muted-foreground">
            {framing.attribution}
          </div>
        )}
      </figure>
    );
  } else if (block.type === "custom_html") {
    const titleText = blockTitle(block);
    bodyNode = (
      <figure id={`block-${block.id}`} className="my-5 space-y-2">
        {(framing || titleText) && (
          <figcaption className="text-sm font-semibold">
            {framing ? framing.label : ""}
            {framing && titleText ? " — " : ""}
            {titleText ?? ""}
          </figcaption>
        )}
        <CustomHtmlView block={block} />
        {framing?.attribution && (
          <div className="text-xs text-muted-foreground">
            {framing.attribution}
          </div>
        )}
      </figure>
    );
  }

  return (
    <div
      data-block-id={block.id}
      className="group/block relative rounded-lg transition-colors hover:bg-black/[0.015] dark:hover:bg-white/[0.02]"
    >
      {!printMode && onOpenCommentForBlock && (
        <button
          type="button"
          title="查看或添加批注"
          aria-label={`为块 ${block.id} 添加或查看批注`}
          onClick={(event) => {
            onOpenCommentForBlock({
              blockId: block.id,
              rect: toViewportRect(event.currentTarget.getBoundingClientRect()),
            });
          }}
          className={`focus-visible:ring-ring absolute top-1 -right-1 z-10 inline-flex items-center gap-1 rounded-full border bg-background px-2 py-0.5 text-[11px] tabular-nums shadow-xs transition-opacity focus-visible:ring-2 focus-visible:outline-none print:hidden ${
            commentCount > 0
              ? "border-amber-500/50 text-amber-700 opacity-100 dark:text-amber-300"
              : "text-muted-foreground opacity-0 group-hover/block:opacity-100 focus-visible:opacity-100"
          }`}
        >
          <MessageSquareIcon className="size-3" />
          {commentCount > 0 && <span>{commentCount}</span>}
        </button>
      )}
      {bodyNode}
    </div>
  );
});

export function ReportViewer({
  document,
  computed,
  viewMode,
  ephemeralPreviews,
  comments = [],
  printMode = false,
  previewMode = false,
  pageFilter,
  onPagesReady,
  onOpenCommentForBlock,
  onAskAgentWithQuote,
}: ReportViewerProps) {
  const suppressChrome = printMode || previewMode;
  const rootRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLDivElement>(null);
  const [measurements, setMeasurements] = useState<PageMeasurements>({
    blockHeights: {},
    tableMeasurements: {},
    coverHeight: 0,
    bibliographyHeight: 0,
  });
  const [pageScale, setPageScale] = useState(1);
  const [quoteAction, setQuoteAction] = useState<FloatingQuoteAction | null>(
    null,
  );

  const isPaged = viewMode === "paged" || printMode;

  useEffect(() => {
    if (!isPaged || suppressChrome) {
      const resetFrame = window.requestAnimationFrame(() => setPageScale(1));
      return () => window.cancelAnimationFrame(resetFrame);
    }
    const el = rootRef.current;
    if (!el) return undefined;

    const updateScale = (width: number) => {
      if (width <= 0) return;
      const next = Math.min(1, Math.max(0.45, (width - 32) / A4_PAGE_WIDTH_PX));
      setPageScale(next);
    };

    const frame = window.requestAnimationFrame(() => {
      updateScale(el.getBoundingClientRect().width);
    });
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        updateScale(entry.contentRect.width);
      }
    });
    observer.observe(el);
    return () => {
      window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [isPaged, suppressChrome]);

  const { openCountByBlock, quotesByBlock } = useMemo(() => {
    const counts = new Map<string, number>();
    const quotes = new Map<string, TextQuoteSelector[]>();
    for (const thread of comments) {
      if (thread.status !== "archived" && !thread.anchor_detached) {
        counts.set(thread.block_id, (counts.get(thread.block_id) ?? 0) + 1);
        if (thread.quote) {
          const list = quotes.get(thread.block_id) ?? [];
          list.push(thread.quote);
          quotes.set(thread.block_id, list);
        }
      }
    }
    return { openCountByBlock: counts, quotesByBlock: quotes };
  }, [comments]);

  const previewList = useMemo(
    () =>
      ephemeralPreviews
        ? Array.from(ephemeralPreviews.values()).filter(
            (p) => p.status === "streaming",
          )
        : [],
    [ephemeralPreviews],
  );

  const layoutSignature = useMemo(
    () =>
      `${document.revision}:${document.blocks
        .map((block) => block.id)
        .join(
          ",",
        )}:${computed.bibliography.length}:${document.cover.title ?? ""}`,
    [
      document.blocks,
      document.revision,
      computed.bibliography.length,
      document.cover.title,
    ],
  );

  useLayoutEffect(() => {
    if (!isPaged) {
      return undefined;
    }
    const container = measureRef.current;
    if (!container || layoutSignature.length === 0) {
      return undefined;
    }

    const blockHeights: Record<string, number> = {};
    const tableMeasurements: Record<string, TableMeasurement> = {};

    const items = container.querySelectorAll<HTMLElement>("[data-measure-id]");
    for (const el of items) {
      const id = el.dataset.measureId;
      if (!id) continue;
      blockHeights[id] = el.getBoundingClientRect().height;

      const theadEl = el.querySelector<HTMLElement>(
        '[data-measure-thead="true"]',
      );
      const tfootEl = el.querySelector<HTMLElement>(
        '[data-measure-tfoot="true"]',
      );
      const rowEls = el.querySelectorAll<HTMLElement>(
        "[data-measure-row-index]",
      );
      if (theadEl || rowEls.length > 0) {
        const rowHeights: number[] = [];
        for (const rowEl of rowEls) {
          rowHeights.push(rowEl.getBoundingClientRect().height);
        }
        tableMeasurements[id] = {
          theadHeight: theadEl ? theadEl.getBoundingClientRect().height : 0,
          tfootHeight: tfootEl ? tfootEl.getBoundingClientRect().height : 0,
          rowHeights,
        };
      }
    }

    const coverEl = container.querySelector<HTMLElement>(
      '[data-measure-cover="true"]',
    );
    const bibEl = container.querySelector<HTMLElement>(
      '[data-measure-bib="true"]',
    );

    setMeasurements({
      blockHeights,
      tableMeasurements,
      coverHeight: coverEl ? coverEl.getBoundingClientRect().height : 0,
      bibliographyHeight: bibEl ? bibEl.getBoundingClientRect().height : 0,
    });
    return undefined;
  }, [layoutSignature, isPaged]);

  const customCss = document.theme.custom_css;

  useEffect(() => {
    if (customCss === undefined || customCss.trim().length === 0) {
      return undefined;
    }
    const style = window.document.createElement("style");
    style.setAttribute("data-quire-theme", document.doc_id);
    style.textContent = customCss;
    window.document.head.appendChild(style);
    return () => {
      style.remove();
    };
  }, [document.doc_id, customCss]);

  const themeVars: Record<string, string> = {};
  for (const [k, v] of Object.entries(document.theme.tokens ?? {})) {
    themeVars[k] = v;
  }

  const scrollToSource = useCallback((sourceId: string) => {
    const el = window.document.getElementById(`bib-${sourceId}`);
    el?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, []);

  const handleMouseUp = useCallback(() => {
    if (suppressChrome || !onOpenCommentForBlock) return;
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
      setQuoteAction(null);
      return;
    }
    const exact = selection.toString().trim();
    if (exact.length === 0) {
      setQuoteAction(null);
      return;
    }
    const range = selection.getRangeAt(0);
    const startBlock = findBlockElement(range.startContainer);
    const endBlock = findBlockElement(range.endContainer);
    if (!startBlock || !endBlock || startBlock !== endBlock) {
      setQuoteAction(null);
      return;
    }
    const blockId = startBlock.dataset.blockId;
    if (!blockId) {
      setQuoteAction(null);
      return;
    }

    const fullText = startBlock.textContent ?? "";
    const matchPos = fullText.indexOf(exact);
    const prefix =
      matchPos > 0
        ? fullText.slice(Math.max(0, matchPos - 24), matchPos)
        : null;
    const suffix =
      matchPos >= 0
        ? fullText.slice(matchPos + exact.length, matchPos + exact.length + 24)
        : null;

    const rect = range.getBoundingClientRect();
    const rootRect = rootRef.current?.getBoundingClientRect();
    const top = rootRect ? rect.top - rootRect.top - 36 : rect.top - 36;
    const left = rootRect
      ? Math.max(8, rect.left - rootRect.left + rect.width / 2 - 56)
      : Math.max(8, rect.left);

    setQuoteAction({
      blockId,
      quote: {
        exact,
        prefix: prefix && prefix.length > 0 ? prefix : undefined,
        suffix: suffix && suffix.length > 0 ? suffix : undefined,
      },
      top,
      left,
      viewportRect: toViewportRect(rect),
    });
  }, [suppressChrome, onOpenCommentForBlock]);

  useEffect(() => {
    window.document.addEventListener("mouseup", handleMouseUp);
    return () => window.document.removeEventListener("mouseup", handleMouseUp);
  }, [handleMouseUp]);

  const renderCover = () => {
    const cover = document.cover;
    if (!cover.enabled || !cover.title) {
      return null;
    }
    const kicker = cover.meta?.kicker;
    const authors = cover.authors ?? [];
    return (
      <header className="mb-8 border-b border-black/15 pb-6 dark:border-white/15">
        {kicker && (
          <div className="mb-2 text-xs font-semibold tracking-wider text-primary uppercase">
            {kicker}
          </div>
        )}
        <h1 className="text-3xl font-bold tracking-tight">{cover.title}</h1>
        {cover.subtitle && (
          <p className="mt-2 text-base text-muted-foreground">
            {cover.subtitle}
          </p>
        )}
        <div className="mt-4 flex flex-wrap gap-4 text-xs text-muted-foreground">
          {authors.length > 0 && <span>作者：{authors.join("、")}</span>}
          {cover.date && <span>日期：{cover.date}</span>}
        </div>
      </header>
    );
  };

  const renderBibliography = () => {
    const references = document.references;
    if (!references.enabled || computed.bibliography.length === 0) {
      return null;
    }
    return (
      <section className="mt-10 border-t border-black/15 pt-6 dark:border-white/15">
        <h2 className="mb-3 text-lg font-semibold">
          {references.title || "参考资料"}
        </h2>
        <ol className="space-y-2.5 pl-5 text-xs leading-relaxed">
          {computed.bibliography.map((entry) => {
            const source = entry.source;
            const authors = source.authors ?? [];
            return (
              <li
                key={entry.source_id}
                id={`bib-${entry.source_id}`}
                className="space-y-0.5"
              >
                <div>
                  <span className="font-medium">{source.title}</span>
                  {authors.length > 0 && (
                    <span className="text-muted-foreground">
                      {" "}
                      — {authors.join(", ")}
                    </span>
                  )}
                  {source.date && (
                    <span className="text-muted-foreground">
                      {" "}
                      ({source.date})
                    </span>
                  )}
                  {source.url && (
                    <a
                      href={source.url}
                      target="_blank"
                      rel="noreferrer noopener"
                      className="ml-2 text-blue-600 underline dark:text-blue-400"
                    >
                      链接
                    </a>
                  )}
                </div>
                {entry.excerpt && (
                  <div className="rounded bg-black/[0.03] px-2 py-1 text-[11px] text-muted-foreground dark:bg-white/[0.04]">
                    “{entry.excerpt}”
                  </div>
                )}
              </li>
            );
          })}
        </ol>
      </section>
    );
  };

  const pages = useMemo(() => {
    if (!isPaged) return [];
    return paginateBlocks(document.blocks, measurements);
  }, [isPaged, document.blocks, measurements]);

  const pagedMixedItems = useMemo(() => {
    if (!isPaged) return [];
    return mixPreviewsIntoPages(
      pages,
      suppressChrome ? [] : previewList,
      document.blocks,
    );
  }, [isPaged, pages, suppressChrome, previewList, document.blocks]);

  const visiblePages = useMemo(() => {
    const entries = pages.map((page, pageIdx) => ({ page, pageIdx }));
    if (pageFilter === undefined) {
      return entries;
    }
    const targetIndex = pageFilter - 1;
    return entries.filter(({ pageIdx }) => pageIdx === targetIndex);
  }, [pages, pageFilter]);

  useEffect(() => {
    if (!isPaged) {
      return undefined;
    }
    onPagesReady?.(pages.length);
    return undefined;
  }, [isPaged, onPagesReady, pages.length]);

  const continuousItems = useMemo(() => {
    if (isPaged) return [];
    const baseSlices: PageSlice[] = document.blocks.map((block) => ({ block }));
    return buildMixedItems(baseSlices, previewList, document.blocks);
  }, [isPaged, document.blocks, previewList]);

  return (
    <div
      ref={rootRef}
      style={themeVars}
      className="report-root relative w-full"
    >
      {quoteAction &&
        (onOpenCommentForBlock || onAskAgentWithQuote) &&
        !suppressChrome && (
          <div
            style={{
              top: `${Math.max(8, quoteAction.top)}px`,
              left: `${quoteAction.left}px`,
            }}
            className="absolute z-40 inline-flex items-center gap-1 rounded-full border bg-background/95 p-1 shadow-md backdrop-blur print:hidden"
          >
            {onOpenCommentForBlock && (
              <button
                type="button"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  onOpenCommentForBlock({
                    blockId: quoteAction.blockId,
                    quote: quoteAction.quote,
                    rect: quoteAction.viewportRect,
                  });
                  setQuoteAction(null);
                  window.getSelection()?.removeAllRanges();
                }}
                className="inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium text-amber-700 hover:bg-amber-500/10 dark:text-amber-300"
              >
                <MessageSquarePlusIcon className="size-3.5" />
                <span>批注</span>
              </button>
            )}
            {onAskAgentWithQuote && (
              <button
                type="button"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  onAskAgentWithQuote(
                    quoteAction.blockId,
                    quoteAction.quote.exact,
                  );
                  setQuoteAction(null);
                  window.getSelection()?.removeAllRanges();
                }}
                className="inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium text-primary hover:bg-primary/10"
              >
                <SparklesIcon className="size-3.5" />
                <span>问问 Agent</span>
              </button>
            )}
          </div>
        )}

      {isPaged && (
        <div
          ref={measureRef}
          aria-hidden="true"
          style={{ width: `${A4_CONTENT_WIDTH_PX}px` }}
          className="pointer-events-none fixed top-0 -left-[99999px] opacity-0"
        >
          <div data-measure-cover="true">{renderCover()}</div>
          {document.blocks.map((block) => (
            <div key={block.id} data-measure-id={block.id}>
              <ReportBlockItem
                slice={{ block }}
                isPaged
                printMode
                computed={computed}
                commentCount={0}
                onCitationClick={scrollToSource}
              />
            </div>
          ))}
          <div data-measure-bib="true">{renderBibliography()}</div>
        </div>
      )}

      {!isPaged ? (
        <article className="mx-auto w-full max-w-[760px] px-5 py-8 sm:px-10">
          {renderCover()}
          {continuousItems.map((item) => (
            <div key={item.key}>
              {item.preview ? (
                <EhemeralBlock preview={item.preview} />
              ) : item.slice ? (
                <ReportBlockItem
                  slice={item.slice}
                  isPaged={false}
                  printMode={suppressChrome}
                  computed={computed}
                  commentCount={openCountByBlock.get(item.slice.block.id) ?? 0}
                  blockQuotes={quotesByBlock.get(item.slice.block.id)}
                  onCitationClick={scrollToSource}
                  onOpenCommentForBlock={onOpenCommentForBlock}
                />
              ) : null}
            </div>
          ))}
          {renderBibliography()}
        </article>
      ) : (
        <div className="flex flex-col items-center gap-6 py-6 print:gap-0 print:py-0">
          {visiblePages.map(({ page, pageIdx }) => (
            <div
              key={page.pageNumber}
              style={suppressChrome ? undefined : { zoom: pageScale }}
              className="relative shrink-0"
            >
              <section
                style={{
                  width: `${A4_PAGE_WIDTH_PX}px`,
                  minHeight: `${A4_PAGE_HEIGHT_PX}px`,
                  padding: `${A4_PADDING_Y_PX}px ${A4_PADDING_X_PX}px`,
                }}
                className="a4-page relative flex flex-col justify-between bg-white text-neutral-900 shadow-md ring-1 ring-black/10 print:shadow-none print:ring-0"
              >
                <div className="flex-1">
                  {page.includeCover && renderCover()}
                  {(pagedMixedItems[pageIdx] ?? []).map((item) => (
                    <div key={item.key}>
                      {item.preview ? (
                        <EhemeralBlock preview={item.preview} />
                      ) : item.slice ? (
                        <ReportBlockItem
                          slice={item.slice}
                          isPaged
                          printMode={suppressChrome}
                          computed={computed}
                          commentCount={
                            openCountByBlock.get(item.slice.block.id) ?? 0
                          }
                          blockQuotes={quotesByBlock.get(item.slice.block.id)}
                          onCitationClick={scrollToSource}
                          onOpenCommentForBlock={onOpenCommentForBlock}
                        />
                      ) : null}
                    </div>
                  ))}
                  {page.includeBibliography && renderBibliography()}
                </div>
                <footer className="mt-4 flex items-center justify-between border-t border-black/10 pt-2 text-[11px] text-neutral-500">
                  <span>{document.cover.title || document.doc_id}</span>
                  <span className="tabular-nums">
                    第 {page.pageNumber} 页 / 共 {pages.length} 页
                  </span>
                </footer>
              </section>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function EhemeralBlock({ preview }: { preview: EphemeralPreviewEvent }) {
  return <EphemeralPreviewCard preview={preview} />;
}

import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  BookOpenIcon,
  ExternalLinkIcon,
  MessageSquareIcon,
} from "lucide-react";
import { CustomHtmlView } from "@/components/report/CustomHtmlView";
import { EchartsCanvas } from "@/components/report/EchartsCanvas";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import {
  InlineMarkdown,
  type QuoteHighlight,
} from "@/components/report/InlineMarkdown";
import {
  SelectionQuoteToolbar,
  type SelectionQuoteAction,
} from "@/components/report/SelectionQuoteToolbar";
import { TableBlockView } from "@/components/report/TableBlockView";
import {
  A4_CONTENT_HEIGHT_PX,
  A4_CONTENT_WIDTH_PX,
  A4_FOOTER_HEIGHT_PX,
  A4_HEADER_HEIGHT_PX,
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
  BibliographyEntry,
  CommentThread,
  ComputedDocumentMeta,
  EphemeralPreviewEvent,
  PageChromeConfig,
} from "@/contracts/report-protocol";
import type { ReportBlock, ReportDocumentView } from "@/lib/document";
import {
  formatPageTemplate,
  hasPageFooter,
  hasPageHeader,
  normalizePageChromeConfig,
  usePageChromeConfig,
} from "@/lib/pageChrome";
import type { ReportViewMode } from "@/lib/preferences";

export interface ReportViewerProps {
  document: ReportDocumentView;
  computed: ComputedDocumentMeta;
  viewMode: ReportViewMode;
  ephemeralPreviews?: Map<string, EphemeralPreviewEvent>;
  comments?: CommentThread[];
  printMode?: boolean;
  previewMode?: boolean;
  pageChrome?: PageChromeConfig;
  pageFilter?: number;
  onPagesReady?: (totalPages: number) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onOpenCommentThread?: (threadId: string, blockId: string) => void;
  onAskAgentWithQuote?: (blockId: string, exact: string) => void;
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

function renderHeadingTag(level: number, blockId: string, content: ReactNode) {
  switch (level) {
    case 1:
      return (
        <h1
          id={`block-${blockId}`}
          className="mt-8 mb-4 border-b-2 border-[var(--rpt-accent)] pb-2 text-xl font-bold tracking-tight text-[var(--rpt-ink)]"
        >
          {content}
        </h1>
      );
    case 2:
      return (
        <h2
          id={`block-${blockId}`}
          className="mt-7 mb-3 border-b border-[var(--rpt-border)] border-l-[3px] border-l-[var(--rpt-accent)] pb-1.5 pl-2.5 text-[17px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h2>
      );
    case 3:
      return (
        <h3
          id={`block-${blockId}`}
          className="mt-5 mb-2 text-[15px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h3>
      );
    case 4:
      return (
        <h4
          id={`block-${blockId}`}
          className="mt-4 mb-1.5 text-[14px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h4>
      );
    default:
      return (
        <h5
          id={`block-${blockId}`}
          className="mt-3 mb-1 text-[13px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h5>
      );
  }
}

function headingPreviewClass(level: number | null | undefined): string {
  switch (level) {
    case 1:
      return "text-xl font-bold tracking-tight";
    case 2:
      return "text-[17px] font-semibold";
    case 3:
      return "text-[15px] font-semibold";
    case 4:
      return "text-[14px] font-semibold";
    default:
      return "text-[13px] font-semibold";
  }
}

function EphemeralPreviewCard({ preview }: { preview: EphemeralPreviewEvent }) {
  const { block } = preview;
  const cursor = (
    <span className="ml-0.5 inline-block h-[1.05em] w-[2px] animate-pulse bg-[var(--rpt-accent,#1E3A5F)]/70 align-text-bottom" />
  );
  const paragraphClass =
    "m-0 whitespace-pre-wrap text-[14.5px] leading-[1.82] text-[var(--rpt-ink,#1F2937)]";

  let body: React.ReactNode;
  if (block.block_type === "heading") {
    body = (
      <div
        className={`font-[family-name:var(--rpt-font-heading)] text-[var(--rpt-ink,#1F2937)] ${headingPreviewClass(block.level)}`}
      >
        {block.text || ""}
        {cursor}
      </div>
    );
  } else if (block.block_type === "quote") {
    body = (
      <blockquote className="m-0 whitespace-pre-wrap bg-slate-50/90 px-4 py-3 text-[14px] leading-[1.75] text-slate-700">
        {block.text || ""}
        {cursor}
      </blockquote>
    );
  } else if (block.block_type === "table") {
    body = block.text ? (
      <p className={paragraphClass}>
        {block.text}
        {cursor}
      </p>
    ) : (
      <div className="space-y-1.5 rounded-lg border border-slate-200/80 bg-slate-100/80 p-2">
        <div className="grid grid-cols-3 gap-2">
          <div className="h-4 animate-pulse rounded bg-slate-200/80" />
          <div className="h-4 animate-pulse rounded bg-slate-200/80" />
          <div className="h-4 animate-pulse rounded bg-slate-200/80" />
        </div>
        <div className="grid grid-cols-3 gap-2">
          <div className="h-3.5 animate-pulse rounded bg-slate-200/60" />
          <div className="h-3.5 animate-pulse rounded bg-slate-200/60" />
          <div className="h-3.5 animate-pulse rounded bg-slate-200/60" />
        </div>
        <div className="grid grid-cols-3 gap-2">
          <div className="h-3.5 animate-pulse rounded bg-slate-200/40" />
          <div className="h-3.5 animate-pulse rounded bg-slate-200/40" />
          <div className="h-3.5 animate-pulse rounded bg-slate-200/40" />
        </div>
      </div>
    );
  } else if (block.block_type === "echarts") {
    body = (
      <div className="flex h-44 items-end justify-around gap-3 rounded-lg border border-slate-200/80 bg-slate-100/80 p-4">
        <div className="h-1/2 w-8 animate-pulse rounded-t bg-slate-200/80" />
        <div className="h-4/5 w-8 animate-pulse rounded-t bg-slate-300/70" />
        <div className="h-2/3 w-8 animate-pulse rounded-t bg-slate-200/70" />
        <div className="h-full w-8 animate-pulse rounded-t bg-slate-300/60" />
        <div className="h-3/5 w-8 animate-pulse rounded-t bg-slate-200/60" />
      </div>
    );
  } else if (block.text) {
    body = (
      <p className={paragraphClass}>
        {block.text}
        {cursor}
      </p>
    );
  } else {
    body = <div className="h-20 animate-pulse rounded-lg bg-slate-100/80" />;
  }

  const status =
    preview.actor === "main" ? "正在撰写" : `${preview.actor} · 撰写中`;

  return (
    <div
      data-preview-id={preview.preview_id}
      data-block-id={block.block_id ?? undefined}
      className="my-3 border-l-2 border-[var(--rpt-accent,#1E3A5F)]/35 pl-3.5 transition-opacity"
    >
      <div className="mb-1.5 flex items-center justify-between gap-2 text-[11px] text-slate-400">
        <span>{status}</span>
        <span className="size-1.5 animate-pulse rounded-full bg-[var(--rpt-accent,#1E3A5F)]/60" />
      </div>
      {block.title && (
        <div className="mb-1.5 font-[family-name:var(--rpt-font-heading)] text-[15px] font-semibold text-[var(--rpt-ink,#1F2937)]">
          {block.title}
        </div>
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

  const sliceIds = new Set(slices.map((s) => s.block.id));
  const afterMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforeMap = new Map<string, EphemeralPreviewEvent[]>();
  const replaceMap = new Map<string, EphemeralPreviewEvent>();
  const tailPreviews: EphemeralPreviewEvent[] = [];

  for (const p of previews) {
    const anchor = p.anchor;
    if (anchor.type === "replace_block" && sliceIds.has(anchor.block_id)) {
      replaceMap.set(anchor.block_id, p);
    } else if (
      anchor.type === "before_block" &&
      sliceIds.has(anchor.block_id)
    ) {
      const list = beforeMap.get(anchor.block_id) ?? [];
      list.push(p);
      beforeMap.set(anchor.block_id, list);
    } else if (anchor.type === "after_block" && sliceIds.has(anchor.block_id)) {
      const list = afterMap.get(anchor.block_id) ?? [];
      list.push(p);
      afterMap.set(anchor.block_id, list);
    } else if (anchor.type === "append_to_section") {
      const targetId =
        sectionLastBlock.get(anchor.section_id) ?? anchor.section_id;
      if (sliceIds.has(targetId)) {
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

function blockNote(block: ReportBlock): string | undefined {
  if (
    block.type !== "table" &&
    block.type !== "echarts" &&
    block.type !== "custom_html"
  ) {
    return undefined;
  }
  const note = block.note;
  return typeof note === "string" ? note : undefined;
}

function BlockFooterNotes({
  block,
  computed,
  bibliographyById,
  printMode,
  onCitationClick,
  align,
  framingAttribution,
}: {
  block: ReportBlock;
  computed: ComputedDocumentMeta;
  bibliographyById: Map<string, BibliographyEntry>;
  printMode: boolean;
  onCitationClick: (sourceId: string) => void;
  align: "left" | "center";
  framingAttribution?: string | null;
}) {
  const note = blockNote(block)?.trim();
  const sourceNote = block.source_note?.trim();
  const sourceIds = block.source_ids ?? [];
  const citedInNote = new Set<string>();
  if (sourceNote) {
    for (const match of sourceNote.matchAll(/\[\^([A-Za-z0-9_-]+)\]/g)) {
      const sourceId = match[1];
      if (sourceId) {
        citedInNote.add(sourceId);
      }
    }
  }
  const missingIds = sourceIds.filter((sourceId) => !citedInNote.has(sourceId));
  const sourceText = [
    sourceNote ?? "",
    ...missingIds.map((sourceId) => `[^${sourceId}]`),
  ]
    .filter((part) => part.length > 0)
    .join(" ");
  const hasSource = sourceText.length > 0;

  const className = `space-y-0.5 pt-1 text-[11.5px] leading-relaxed text-slate-500 dark:text-slate-400 ${
    align === "center" ? "text-center" : "text-left"
  }`;

  if (!note && !hasSource) {
    return framingAttribution ? (
      <div className={className}>{framingAttribution}</div>
    ) : null;
  }

  return (
    <div className={className}>
      {note && (
        <div>
          注：
          <InlineMarkdown
            text={note}
            citationMap={computed.citation_map}
            bibliographyById={bibliographyById}
            printMode={printMode}
            onCitationClick={onCitationClick}
          />
        </div>
      )}
      {hasSource && (
        <div>
          资料来源：
          <InlineMarkdown
            text={sourceText}
            citationMap={computed.citation_map}
            bibliographyById={bibliographyById}
            printMode={printMode}
            onCitationClick={onCitationClick}
          />
        </div>
      )}
    </div>
  );
}

function referenceDomain(url: string | null | undefined): string | null {
  if (!url) {
    return null;
  }
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return null;
  }
}

// Scale the fixed-width A4 page with a compositor transform. CSS `zoom` would
// reflow text at fractional scales, making the page height and line wrapping
// jitter while the panel is resized. The outer box reserves the scaled size.
function ScaledPage({
  scale,
  plain = false,
  children,
}: {
  scale: number;
  plain?: boolean;
  children: ReactNode;
}) {
  const outerRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (plain) {
      return undefined;
    }
    const outer = outerRef.current;
    const inner = innerRef.current;
    if (!outer || !inner) {
      return undefined;
    }
    const apply = () => {
      outer.style.width = `${A4_PAGE_WIDTH_PX * scale}px`;
      outer.style.height = `${inner.offsetHeight * scale}px`;
    };
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(inner);
    return () => observer.disconnect();
  }, [scale, plain]);

  if (plain) {
    return <>{children}</>;
  }

  return (
    <div ref={outerRef} className="relative shrink-0">
      <div
        ref={innerRef}
        style={{
          width: `${A4_PAGE_WIDTH_PX}px`,
          transform: `scale(${scale})`,
          transformOrigin: "top left",
        }}
      >
        {children}
      </div>
    </div>
  );
}

function CoverCustomHtml({ html }: { html: string }) {
  const setHost = useCallback(
    (node: HTMLDivElement | null) => {
      if (node) {
        node.innerHTML = html;
      }
    },
    [html],
  );

  return <div ref={setHost} className="report-custom-cover-html" />;
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
  showChartTools?: boolean;
  computed: ComputedDocumentMeta;
  bibliographyById: Map<string, BibliographyEntry>;
  commentCount: number;
  blockQuotes?: QuoteHighlight[];
  onCitationClick: (sourceId: string) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onOpenCommentThread?: (threadId: string, blockId: string) => void;
}

const ReportBlockItem = memo(function ReportBlockItem({
  slice,
  isPaged,
  printMode,
  showChartTools = true,
  computed,
  bibliographyById,
  commentCount,
  blockQuotes,
  onCitationClick,
  onOpenCommentForBlock,
  onOpenCommentThread,
}: ReportBlockItemProps) {
  const { block } = slice;
  const framing = computed.block_framing[block.id];

  const handleQuoteClick = useCallback(
    (highlight: QuoteHighlight, rect: DOMRect) => {
      if (onOpenCommentThread) {
        onOpenCommentThread(highlight.threadId, block.id);
        return;
      }
      if (!onOpenCommentForBlock) return;
      onOpenCommentForBlock({
        blockId: block.id,
        quote: highlight.quote,
        rect: toViewportRect(rect),
      });
    },
    [block.id, onOpenCommentForBlock, onOpenCommentThread],
  );
  const quoteHandler =
    !printMode && (onOpenCommentForBlock || onOpenCommentThread)
      ? handleQuoteClick
      : undefined;

  const renderInline = (text: string) => (
    <InlineMarkdown
      text={text}
      citationMap={computed.citation_map}
      bibliographyById={bibliographyById}
      printMode={printMode}
      onCitationClick={onCitationClick}
      highlightQuotes={blockQuotes}
      onQuoteClick={quoteHandler}
    />
  );

  let bodyNode: React.ReactNode = null;
  if (block.type === "heading") {
    bodyNode = renderHeadingTag(
      block.level ?? 2,
      block.id,
      renderInline(block.text),
    );
  } else if (block.type === "paragraph") {
    bodyNode = (
      <p
        id={`block-${block.id}`}
        className="my-3 text-[14.5px] leading-[1.82] tracking-[0.01em] text-[var(--rpt-ink)]"
      >
        {renderInline(block.text)}
      </p>
    );
  } else if (block.type === "quote") {
    bodyNode = (
      <blockquote
        id={`block-${block.id}`}
        className="my-4 rounded-r-lg border-l-[3px] border-[var(--rpt-accent)] bg-slate-50/90 px-4 py-3 text-[14px] leading-[1.75] text-slate-700"
      >
        {renderInline(block.text)}
      </blockquote>
    );
  } else if (block.type === "table") {
    const titleText = block.title?.trim();
    const totalRows =
      slice.totalRows ?? (Array.isArray(block.rows) ? block.rows.length : 0);
    const captionPrefix = framing?.label ?? "";
    const captionText = [captionPrefix, titleText]
      .filter((part): part is string => Boolean(part))
      .join("  ");
    const showCaption = Boolean(captionText || slice.continuation);

    bodyNode = (
      <figure id={`block-${block.id}`} className="my-5 space-y-2">
        {showCaption && (
          <figcaption className="text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800 dark:text-slate-100">
            {captionText}
            {slice.continuation ? "（续）" : ""}
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
        {slice.continuation &&
          slice.rowStart !== undefined &&
          slice.rowEnd !== undefined && (
            <div className="text-right text-[11px] font-normal text-slate-500 tabular-nums dark:text-slate-400">
              接上页 · 第 {slice.rowStart + 1}–{slice.rowEnd} 行 / 共{" "}
              {totalRows} 行
            </div>
          )}
        {slice.includeSummary !== false && (
          <BlockFooterNotes
            block={block}
            computed={computed}
            bibliographyById={bibliographyById}
            printMode={printMode}
            onCitationClick={onCitationClick}
            align="left"
            framingAttribution={framing?.attribution}
          />
        )}
      </figure>
    );
  } else if (block.type === "echarts") {
    const titleText = blockTitle(block);
    const captionPrefix = framing?.label ?? "";
    const captionText = [captionPrefix, titleText]
      .filter((part): part is string => Boolean(part))
      .join("  ");
    bodyNode = (
      <figure id={`block-${block.id}`} className="my-5 space-y-2">
        <EchartsCanvas
          blockId={block.id}
          chart={block}
          chartTables={computed.chart_tables?.[block.id]}
          requirements={computed.chart_requirements?.[block.id]}
          renderer={isPaged || printMode ? "svg" : "canvas"}
          printSafe={isPaged || printMode}
          showTools={showChartTools}
        />
        {captionText && (
          <figcaption className="pt-1 text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800 dark:text-slate-100">
            {captionText}
          </figcaption>
        )}
        <BlockFooterNotes
          block={block}
          computed={computed}
          bibliographyById={bibliographyById}
          printMode={printMode}
          onCitationClick={onCitationClick}
          align="center"
          framingAttribution={framing?.attribution}
        />
      </figure>
    );
  } else if (block.type === "custom_html") {
    const titleText = blockTitle(block);
    const captionPrefix = framing?.label ?? "";
    const captionText = [captionPrefix, titleText]
      .filter((part): part is string => Boolean(part))
      .join("  ");
    bodyNode = (
      <figure id={`block-${block.id}`} className="my-5 space-y-2">
        <CustomHtmlView block={block} />
        {captionText && (
          <figcaption className="pt-1 text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800 dark:text-slate-100">
            {captionText}
          </figcaption>
        )}
        <BlockFooterNotes
          block={block}
          computed={computed}
          bibliographyById={bibliographyById}
          printMode={printMode}
          onCitationClick={onCitationClick}
          align="center"
          framingAttribution={framing?.attribution}
        />
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
          className={`focus-visible:ring-ring absolute top-1 left-full z-10 ml-2 inline-flex items-center gap-1 rounded-full border bg-background px-2 py-0.5 text-[11px] tabular-nums shadow-xs transition-opacity focus-visible:ring-2 focus-visible:outline-none print:hidden max-md:top-1 max-md:right-1 max-md:left-auto max-md:ml-0 ${
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
  pageChrome,
  pageFilter,
  onPagesReady,
  onOpenCommentForBlock,
  onOpenCommentThread,
  onAskAgentWithQuote,
}: ReportViewerProps) {
  const suppressChrome = printMode || previewMode;
  const resolvedPageChrome = usePageChromeConfig(
    document.doc_id,
    document.theme.page_chrome,
  );
  const effectivePageChrome = pageChrome
    ? normalizePageChromeConfig(pageChrome)
    : resolvedPageChrome;
  const effectivePageHeight =
    A4_CONTENT_HEIGHT_PX -
    (hasPageHeader(effectivePageChrome) ? A4_HEADER_HEIGHT_PX : 0) -
    (hasPageFooter(effectivePageChrome) ? A4_FOOTER_HEIGHT_PX : 0);
  const showFooter = hasPageFooter(effectivePageChrome);
  const rootRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLDivElement>(null);
  const [measurements, setMeasurements] = useState<PageMeasurements>({
    blockHeights: {},
    tableMeasurements: {},
    coverHeight: 0,
    bibliographyHeight: 0,
  });
  const [pageScale, setPageScale] = useState(1);
  const [quoteAction, setQuoteAction] = useState<SelectionQuoteAction | null>(
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
    const quotes = new Map<string, QuoteHighlight[]>();
    for (const thread of comments) {
      if (thread.status !== "archived" && !thread.anchor_detached) {
        counts.set(thread.block_id, (counts.get(thread.block_id) ?? 0) + 1);
        if (thread.quote) {
          const list = quotes.get(thread.block_id) ?? [];
          list.push({ threadId: thread.thread_id, quote: thread.quote });
          quotes.set(thread.block_id, list);
        }
      }
    }
    return { openCountByBlock: counts, quotesByBlock: quotes };
  }, [comments]);

  const previewList = useMemo(
    () =>
      !isPaged && ephemeralPreviews
        ? Array.from(ephemeralPreviews.values()).filter(
            (p) => p.status === "streaming",
          )
        : [],
    [ephemeralPreviews, isPaged],
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

  useEffect(() => {
    if (!isPaged) {
      return undefined;
    }
    let cancelled = false;

    const measure = () => {
      const container = measureRef.current;
      if (!container || layoutSignature.length === 0) {
        return;
      }

      const blockHeights: Record<string, number> = {};
      const tableMeasurements: Record<string, TableMeasurement> = {};

      const items = container.querySelectorAll<HTMLElement>("[data-measure-id]");
      for (const el of items) {
        const id = el.dataset.measureId;
        if (!id) continue;
        const blockHeight = el.getBoundingClientRect().height;
        blockHeights[id] = blockHeight;

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
          const tfootHeight = tfootEl
            ? tfootEl.getBoundingClientRect().height
            : 0;
          const rowsHeight = rowHeights.reduce(
            (sum, height) => sum + height,
            0,
          );
          tableMeasurements[id] = {
            theadHeight: theadEl ? theadEl.getBoundingClientRect().height : 0,
            tfootHeight,
            rowHeights,
            chromeHeight: Math.max(0, blockHeight - rowsHeight - tfootHeight),
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
    };

    void (async () => {
      await window.document.fonts.ready;
      await new Promise<void>((resolve) => {
        window.requestAnimationFrame(() => {
          window.requestAnimationFrame(() => resolve());
        });
      });
      if (!cancelled) {
        measure();
      }
    })();

    return () => {
      cancelled = true;
    };
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
  const themeTokens = document.theme.tokens ?? {};
  for (const [k, v] of Object.entries(themeTokens)) {
    themeVars[k] = v;
  }
  if (
    themeTokens["--rpt-accent"] === undefined &&
    themeTokens["--rpt-primary"] !== undefined
  ) {
    themeVars["--rpt-accent"] = themeTokens["--rpt-primary"];
  }

  const bibliographyById = useMemo(() => {
    const map = new Map<string, BibliographyEntry>();
    for (const entry of computed.bibliography) {
      map.set(entry.source_id, entry);
    }
    return map;
  }, [computed.bibliography]);

  const scrollToSource = useCallback((sourceId: string) => {
    const el = window.document.getElementById(`bib-${sourceId}`);
    if (!el) {
      return;
    }
    el.scrollIntoView({ behavior: "smooth", block: "center" });
    el.classList.add("quire-flash-highlight");
    window.setTimeout(() => {
      el.classList.remove("quire-flash-highlight");
    }, 1600);
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

    setQuoteAction({
      blockId,
      quote: {
        exact,
        prefix: prefix && prefix.length > 0 ? prefix : undefined,
        suffix: suffix && suffix.length > 0 ? suffix : undefined,
      },
      viewportRect: toViewportRect(rect),
    });
  }, [suppressChrome, onOpenCommentForBlock]);

  useEffect(() => {
    window.document.addEventListener("mouseup", handleMouseUp);
    return () => window.document.removeEventListener("mouseup", handleMouseUp);
  }, [handleMouseUp]);

  const renderCover = () => {
    const cover = document.cover;
    if (!cover.enabled) {
      return null;
    }
    const authors = cover.authors ?? [];
    if (cover.custom_html) {
      let html = cover.custom_html;
      const replacements: Record<string, string> = {
        title: cover.title ?? "",
        subtitle: cover.subtitle ?? "",
        date: cover.date ?? "",
        signature: cover.meta?.signature?.trim() || authors.join("、"),
      };
      for (const [key, value] of Object.entries(replacements)) {
        html = html.replaceAll(`{{${key}}}`, value);
      }
      return (
        <div className="mb-10">
          <CoverCustomHtml html={html} />
        </div>
      );
    }
    if (!cover.title) {
      return null;
    }

    const variant = cover.variant ?? "classic";
    const kicker = cover.meta?.kicker?.trim();
    const unit =
      cover.meta?.unit?.trim() ||
      cover.meta?.organization?.trim() ||
      cover.meta?.institution?.trim() ||
      null;
    const metaEntries = [
      authors.length > 0 ? `作者：${authors.join("、")}` : null,
      unit ? `单位：${unit}` : null,
      cover.date ? `日期：${cover.date}` : null,
    ].filter((entry): entry is string => entry !== null);
    const metaClass =
      variant === "modern"
        ? "rounded-full border border-[var(--rpt-border)] bg-[var(--rpt-surface)] px-2.5 py-0.5"
        : "";
    const renderMeta = (className: string) =>
      metaEntries.length > 0 ? (
        <div
          className={`flex flex-wrap gap-x-5 gap-y-1 text-xs text-[var(--rpt-muted)] ${className}`}
        >
          {metaEntries.map((entry) => (
            <span key={entry} className={metaClass}>
              {entry}
            </span>
          ))}
        </div>
      ) : null;

    if (variant === "modern") {
      return (
        <header className="mb-10 flex gap-4 border-b border-[var(--rpt-border)] pb-6">
          <div className="w-1 shrink-0 rounded-full bg-[var(--rpt-accent)]" />
          <div className="min-w-0 flex-1">
            {kicker && (
              <div className="mb-2 text-[11px] font-semibold tracking-[0.2em] text-[var(--rpt-accent)] uppercase">
                {kicker}
              </div>
            )}
            <h1 className="text-[26px] leading-snug font-bold tracking-tight text-[var(--rpt-ink)] sm:text-[28px]">
              {cover.title}
            </h1>
            {cover.subtitle && (
              <p className="mt-2 text-sm text-[var(--rpt-muted)]">
                {cover.subtitle}
              </p>
            )}
            {renderMeta("mt-4")}
          </div>
        </header>
      );
    }

    if (variant === "minimal") {
      return (
        <header className="mb-10 border-b border-[var(--rpt-border)] pb-5">
          {kicker && (
            <div className="mb-2 text-[11px] font-medium tracking-[0.24em] text-[var(--rpt-muted)] uppercase">
              {kicker}
            </div>
          )}
          <h1 className="text-2xl leading-snug font-semibold tracking-tight text-[var(--rpt-ink)]">
            {cover.title}
          </h1>
          {cover.subtitle && (
            <p className="mt-2 text-sm text-[var(--rpt-muted)]">
              {cover.subtitle}
            </p>
          )}
          {renderMeta("mt-3")}
        </header>
      );
    }

    if (variant === "split") {
      return (
        <header className="mb-10 grid gap-5 border-b-2 border-[var(--rpt-accent)] pb-6 sm:grid-cols-[1.6fr_1fr] sm:gap-8">
          <div className="min-w-0">
            {kicker && (
              <div className="mb-2 text-[11px] font-semibold tracking-[0.22em] text-[var(--rpt-accent)] uppercase">
                {kicker}
              </div>
            )}
            <h1 className="text-[26px] leading-snug font-bold tracking-tight text-[var(--rpt-ink)] sm:text-[28px]">
              {cover.title}
            </h1>
            {cover.subtitle && (
              <p className="mt-2 text-sm text-[var(--rpt-muted)]">
                {cover.subtitle}
              </p>
            )}
          </div>
          <div className="flex flex-col justify-end gap-y-1 border-[var(--rpt-border)] text-xs text-[var(--rpt-muted)] sm:border-l sm:pl-6">
            {metaEntries.map((entry) => (
              <span key={entry}>{entry}</span>
            ))}
          </div>
        </header>
      );
    }

    return (
      <header className="mb-10 text-center">
        {kicker && (
          <div className="mb-2 text-[11px] font-semibold tracking-[0.28em] text-[var(--rpt-accent)] uppercase">
            {kicker}
          </div>
        )}
        <h1 className="mx-auto max-w-[92%] text-[26px] leading-snug font-bold tracking-tight text-[var(--rpt-ink)] sm:text-[28px]">
          {cover.title}
        </h1>
        {cover.subtitle && (
          <p className="mt-3 text-sm text-[var(--rpt-muted)]">
            —— {cover.subtitle} ——
          </p>
        )}
        {renderMeta("mt-5 justify-center")}
        <div className="mt-6 border-t border-[var(--rpt-border)]" />
        <div className="mt-1 border-t-2 border-[var(--rpt-accent)]" />
      </header>
    );
  };

  const renderBibliography = () => {
    const references = document.references;
    if (!references.enabled || computed.bibliography.length === 0) {
      return null;
    }
    return (
      <section className="mt-10 border-t border-[var(--rpt-border)] pt-6">
        <h2 className="mb-4 flex items-center gap-2 text-base font-semibold text-[var(--rpt-ink)]">
          <BookOpenIcon className="size-4 text-[var(--rpt-accent)]" />
          <span>{references.title || "参考资料"}</span>
        </h2>
        <ol className="space-y-3.5 text-xs leading-relaxed">
          {computed.bibliography.map((entry) => {
            const source = entry.source;
            const attribution =
              source.publisher?.trim() || source.authors?.join(", ") || null;
            const domain = referenceDomain(source.url);
            const badge = entry.label.trim() || `[${entry.number}]`;
            const hasMetadata = Boolean(
              attribution || source.date || source.url,
            );
            return (
              <li
                key={entry.source_id}
                id={`bib-${entry.source_id}`}
                className="flex scroll-mt-24 gap-3"
              >
                <span className="mt-px shrink-0 font-mono text-[11px] text-muted-foreground tabular-nums">
                  {badge}
                </span>
                <div className="min-w-0 flex-1 space-y-0.5">
                  {source.url ? (
                    <a
                      href={source.url}
                      target="_blank"
                      rel="noreferrer noopener"
                      className="font-medium text-[var(--rpt-ink)] underline-offset-2 hover:text-[var(--rpt-accent)] hover:underline"
                    >
                      {source.title}
                    </a>
                  ) : (
                    <div className="font-medium text-foreground">
                      {source.title}
                    </div>
                  )}
                  {hasMetadata && (
                    <div className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[11px] text-muted-foreground">
                      {attribution && <span>{attribution}</span>}
                      {attribution && (source.date || source.url) && (
                        <span aria-hidden="true">·</span>
                      )}
                      {source.date && <span>{source.date}</span>}
                      {source.date && source.url && (
                        <span aria-hidden="true">·</span>
                      )}
                      {source.url && (
                        <a
                          href={source.url}
                          target="_blank"
                          rel="noreferrer noopener"
                          className="inline-flex items-center gap-0.5 text-slate-600 underline decoration-slate-400 hover:text-[var(--rpt-accent)]"
                        >
                          {domain ?? "链接"}
                          <ExternalLinkIcon className="size-2.5" />
                        </a>
                      )}
                    </div>
                  )}
                  {entry.excerpt && (
                    <div className="mt-1 rounded border-l-2 border-slate-300 bg-slate-50 px-2 py-1 text-[11px] leading-relaxed text-slate-600">
                      “{entry.excerpt}”
                    </div>
                  )}
                </div>
              </li>
            );
          })}
        </ol>
      </section>
    );
  };

  const pages = useMemo(() => {
    if (!isPaged) return [];
    return paginateBlocks(document.blocks, measurements, effectivePageHeight);
  }, [isPaged, document.blocks, measurements, effectivePageHeight]);

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
      data-theme-preset={document.theme.preset ?? "default"}
      className="report-root relative w-full"
    >
      {quoteAction &&
        (onOpenCommentForBlock || onAskAgentWithQuote) &&
        !suppressChrome && (
          <SelectionQuoteToolbar
            action={quoteAction}
            onComment={
              onOpenCommentForBlock
                ? (action) => {
                    onOpenCommentForBlock({
                      blockId: action.blockId,
                      quote: action.quote,
                      rect: action.viewportRect,
                    });
                    setQuoteAction(null);
                    window.getSelection()?.removeAllRanges();
                  }
                : undefined
            }
            onAskAgent={
              onAskAgentWithQuote
                ? (blockId, exact) => {
                    onAskAgentWithQuote(blockId, exact);
                    setQuoteAction(null);
                    window.getSelection()?.removeAllRanges();
                  }
                : undefined
            }
            onDismiss={() => setQuoteAction(null)}
          />
        )}

      {isPaged && (
        <div
          ref={measureRef}
          aria-hidden="true"
          style={{ width: `${A4_CONTENT_WIDTH_PX}px` }}
          className="report-root pointer-events-none absolute top-0 -left-[99999px] opacity-0 print:hidden"
        >
          <div data-measure-cover="true" className="flow-root">
            {renderCover()}
          </div>
          {document.blocks.map((block) => (
            <div key={block.id} data-measure-id={block.id} className="flow-root">
              <ReportBlockItem
                slice={{ block }}
                isPaged
                printMode
                showChartTools
                computed={computed}
                bibliographyById={bibliographyById}
                commentCount={0}
                blockQuotes={quotesByBlock.get(block.id)}
                onCitationClick={scrollToSource}
              />
            </div>
          ))}
          <div data-measure-bib="true" className="flow-root">
            {renderBibliography()}
          </div>
        </div>
      )}

      {!isPaged ? (
        <article className="mx-auto w-full max-w-[760px] bg-white px-5 py-8 text-[var(--rpt-ink)] shadow-sm ring-1 ring-black/5 sm:px-10">
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
                  showChartTools={!suppressChrome}
                  computed={computed}
                  bibliographyById={bibliographyById}
                  commentCount={openCountByBlock.get(item.slice.block.id) ?? 0}
                  blockQuotes={quotesByBlock.get(item.slice.block.id)}
                  onCitationClick={scrollToSource}
                  onOpenCommentForBlock={onOpenCommentForBlock}
                  onOpenCommentThread={onOpenCommentThread}
                />
              ) : null}
            </div>
          ))}
          {renderBibliography()}
        </article>
      ) : (
        <div className="flex flex-col items-center gap-6 py-6 print:block print:w-full print:gap-0 print:p-0 print:m-0">
          {visiblePages.map(({ page, pageIdx }) => {
            const pageContext = {
              page: page.pageNumber,
              total: pages.length,
              title: document.cover.title || document.doc_id,
              subtitle: document.cover.subtitle ?? "",
              date: document.cover.date ?? "",
              kicker: document.cover.meta?.kicker ?? "",
              authors: (document.cover.authors ?? []).join("、"),
            };
            return (
              <ScaledPage
                key={page.pageNumber}
                scale={suppressChrome ? 1 : pageScale}
                plain={suppressChrome}
              >
                <section
                  style={{
                    width: `${A4_PAGE_WIDTH_PX}px`,
                    height: `${A4_PAGE_HEIGHT_PX}px`,
                    padding: `${A4_PADDING_Y_PX}px ${A4_PADDING_X_PX}px`,
                  }}
                  className="a4-page relative flex flex-col overflow-hidden bg-white text-[var(--rpt-ink)] shadow-md ring-1 ring-black/10 print:shadow-none print:ring-0"
                >
                  <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
                    {hasPageHeader(effectivePageChrome) &&
                      (pageIdx > 0 ||
                        !effectivePageChrome.hide_header_on_first_page) && (
                        <div className="mb-3 grid shrink-0 grid-cols-3 items-center gap-2 border-b border-[var(--rpt-border)] pb-1 text-[10px] tracking-[0.06em] text-[var(--rpt-muted)]">
                          <div className="truncate text-left">
                            {formatPageTemplate(
                              effectivePageChrome.header.left,
                              pageContext,
                            )}
                          </div>
                          <div className="truncate text-center">
                            {formatPageTemplate(
                              effectivePageChrome.header.center,
                              pageContext,
                            )}
                          </div>
                          <div className="truncate text-right">
                            {formatPageTemplate(
                              effectivePageChrome.header.right,
                              pageContext,
                            )}
                          </div>
                        </div>
                      )}
                    <div className="min-h-0 flex-1 overflow-hidden">
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
                              showChartTools={!suppressChrome}
                              computed={computed}
                              bibliographyById={bibliographyById}
                              commentCount={
                                openCountByBlock.get(item.slice.block.id) ?? 0
                              }
                              blockQuotes={quotesByBlock.get(item.slice.block.id)}
                              onCitationClick={scrollToSource}
                              onOpenCommentForBlock={onOpenCommentForBlock}
                              onOpenCommentThread={onOpenCommentThread}
                            />
                          ) : null}
                        </div>
                      ))}
                      {page.includeBibliography && renderBibliography()}
                    </div>
                  </div>
                  {showFooter &&
                    (pageIdx > 0 ||
                      !effectivePageChrome.hide_footer_on_first_page) && (
                      <footer className="mt-4 grid shrink-0 grid-cols-3 items-center gap-2 border-t border-[var(--rpt-border)] pt-2 text-[11px] text-[var(--rpt-muted)]">
                        <span className="truncate text-left">
                          {formatPageTemplate(
                            effectivePageChrome.footer.left,
                            pageContext,
                          )}
                        </span>
                        <span className="truncate text-center tabular-nums">
                          {formatPageTemplate(
                            effectivePageChrome.footer.center,
                            pageContext,
                          )}
                        </span>
                        <span className="truncate text-right tabular-nums">
                          {formatPageTemplate(
                            effectivePageChrome.footer.right,
                            pageContext,
                          )}
                        </span>
                      </footer>
                    )}
                </section>
              </ScaledPage>
            );
          })}
        </div>
      )}
    </div>
  );
}

function EhemeralBlock({ preview }: { preview: EphemeralPreviewEvent }) {
  return <EphemeralPreviewCard preview={preview} />;
}

import { memo, useCallback, type ReactNode } from "react";
import { MessageSquareIcon } from "lucide-react";
import { CustomHtmlView } from "@/components/report/CustomHtmlView";
import { EchartsCanvas } from "@/components/report/EchartsCanvas";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import {
  InlineMarkdown,
  type QuoteHighlight,
} from "@/components/report/InlineMarkdown";
import { TableBlockView } from "@/components/report/TableBlockView";
import {
  A4_CONTENT_WIDTH_PX,
  type PageSlice,
} from "@/components/report/pagination";
import { toViewportRect } from "@/components/report/reportDom";
import type {
  BibliographyEntry,
  ComputedDocumentMeta,
} from "@/contracts/report-protocol";
import type { ReportBlock } from "@/lib/document";

function renderHeadingTag(
  level: number,
  domId: string | undefined,
  content: ReactNode,
) {
  switch (level) {
    case 1:
      return (
        <h1
          id={domId}
          className="mt-8 mb-4 border-b-2 border-[var(--rpt-accent)] pb-2 text-xl font-bold tracking-tight text-[var(--rpt-ink)]"
        >
          {content}
        </h1>
      );
    case 2:
      return (
        <h2
          id={domId}
          className="mt-7 mb-3 border-b border-[var(--rpt-border)] border-l-[3px] border-l-[var(--rpt-accent)] pb-1.5 pl-2.5 text-[17px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h2>
      );
    case 3:
      return (
        <h3
          id={domId}
          className="mt-5 mb-2 text-[15px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h3>
      );
    case 4:
      return (
        <h4
          id={domId}
          className="mt-4 mb-1.5 text-[14px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h4>
      );
    default:
      return (
        <h5
          id={domId}
          className="mt-3 mb-1 text-[13px] font-semibold text-[var(--rpt-ink)]"
        >
          {content}
        </h5>
      );
  }
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

  const className = `space-y-0.5 pt-1 text-[11.5px] leading-relaxed text-slate-500 ${
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

export interface ReportBlockItemProps {
  slice: PageSlice;
  isPaged: boolean;
  printMode: boolean;
  showChartTools?: boolean;
  computed: ComputedDocumentMeta;
  bibliographyById: Map<string, BibliographyEntry>;
  commentCount: number;
  blockQuotes?: QuoteHighlight[];
  measurementOnly?: boolean;
  onCitationClick: (sourceId: string) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onOpenCommentThread?: (threadId: string, blockId: string) => void;
}

export const ReportBlockItem = memo(function ReportBlockItem({
  slice,
  isPaged,
  printMode,
  showChartTools = true,
  computed,
  bibliographyById,
  commentCount,
  blockQuotes,
  measurementOnly = false,
  onCitationClick,
  onOpenCommentForBlock,
  onOpenCommentThread,
}: ReportBlockItemProps) {
  const { block } = slice;
  const framing = computed.block_framing[block.id];
  const domId = measurementOnly ? undefined : `block-${block.id}`;

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

  let bodyNode: ReactNode = null;
  if (block.type === "heading") {
    bodyNode = renderHeadingTag(
      block.level ?? 2,
      domId,
      renderInline(block.text),
    );
  } else if (block.type === "paragraph") {
    bodyNode = (
      <p
        id={domId}
        className="my-3 text-[14.5px] leading-[1.82] tracking-[0.01em] text-[var(--rpt-ink)]"
      >
        {renderInline(block.text)}
      </p>
    );
  } else if (block.type === "quote") {
    bodyNode = (
      <blockquote
        id={domId}
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
      <figure id={domId} className="my-5 space-y-2">
        {showCaption && (
          <figcaption className="text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800">
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
          containerWidthPx={isPaged ? A4_CONTENT_WIDTH_PX : undefined}
        />
        {slice.continuation &&
          slice.rowStart !== undefined &&
          slice.rowEnd !== undefined && (
            <div className="text-right text-[11px] font-normal text-slate-500 tabular-nums">
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
      <figure id={domId} className="my-5 space-y-2">
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
          <figcaption className="pt-1 text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800">
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
      <figure id={domId} className="my-5 space-y-2">
        <CustomHtmlView block={block} />
        {captionText && (
          <figcaption className="pt-1 text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800">
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
      className="group/block relative rounded-lg transition-colors hover:bg-black/[0.015]"
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
              ? "border-amber-500/50 text-amber-700 opacity-100"
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

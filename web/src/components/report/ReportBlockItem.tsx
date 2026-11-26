import { memo, type CSSProperties, type ReactNode } from "react";
import { MessageSquareIcon } from "lucide-react";
import { CustomHtmlView } from "@/components/report/CustomHtmlView";
import { EchartsCanvas } from "@/components/report/EchartsCanvas";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import { InlineMarkdown } from "@/components/report/InlineMarkdown";
import { TableBlockView } from "@/components/report/TableBlockView";
import {
  A4_CONTENT_WIDTH_PX,
  type PageSlice,
} from "@/components/report/pagination";
import { toViewportRect } from "@/components/report/reportDom";
import type {
  BibliographyEntry,
  BlockFraming,
  ChartDataTable,
  ChartRequirements,
  CitationMapEntry,
  TableLayout,
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
          data-rpt-h="1"
          style={{
            fontSize: "var(--rpt-h1-size, 22px)",
            fontWeight:
              "var(--rpt-heading-weight, 700)" as CSSProperties["fontWeight"],
            letterSpacing: "var(--rpt-heading-tracking, -0.01em)",
          }}
          className="rpt-h1 mt-8 mb-4 border-b-2 border-[var(--rpt-accent)] pb-2 text-xl font-bold tracking-tight text-balance text-[var(--rpt-ink)]"
        >
          {content}
        </h1>
      );
    case 2:
      return (
        <h2
          id={domId}
          data-rpt-h="2"
          style={{
            fontWeight:
              "var(--rpt-heading-weight, 600)" as CSSProperties["fontWeight"],
            letterSpacing: "var(--rpt-heading-tracking, 0em)",
          }}
          className="rpt-h2 mt-7 mb-3 border-b border-[var(--rpt-border)] border-l-[3px] border-l-[var(--rpt-accent)] pb-1.5 pl-2.5 text-[17px] font-semibold text-balance text-[var(--rpt-ink)]"
        >
          {content}
        </h2>
      );
    case 3:
      return (
        <h3
          id={domId}
          data-rpt-h="3"
          className="rpt-h3 mt-5 mb-2 text-[15px] font-semibold text-balance text-[var(--rpt-ink)]"
        >
          {content}
        </h3>
      );
    case 4:
      return (
        <h4
          id={domId}
          data-rpt-h="4"
          className="rpt-h4 mt-4 mb-1.5 text-[14px] font-semibold text-balance text-[var(--rpt-ink)]"
        >
          {content}
        </h4>
      );
    default:
      return (
        <h5
          id={domId}
          data-rpt-h="5"
          className="rpt-h5 mt-3 mb-1 text-[13px] font-semibold text-balance text-[var(--rpt-ink)]"
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
  citationMap,
  bibliographyById,
  printMode,
  onCitationClick,
  align,
  framingAttribution,
}: {
  block: ReportBlock;
  citationMap: Record<string, CitationMapEntry>;
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
  const autoSourceNames = !sourceNote
    ? Array.from(
        new Set(
          sourceIds
            .map((sourceId) => {
              const source = bibliographyById.get(sourceId)?.source;
              return source?.publisher?.trim() || source?.title?.trim() || "";
            })
            .filter((name) => name.length > 0),
        ),
      )
        .slice(0, 3)
        .join("、")
    : "";
  const sourceText = [
    sourceNote || autoSourceNames,
    ...missingIds.map((sourceId) => `[^${sourceId}]`),
  ]
    .filter((part) => part.length > 0)
    .join(" ");
  const hasSource = sourceText.length > 0;

  const className = `space-y-0.5 pt-1 text-[11.5px] leading-relaxed text-[var(--rpt-muted)] ${
    align === "center" ? "text-center" : "text-left"
  }`;

  if (!note && !hasSource) {
    return framingAttribution ? (
      <div data-measure-notes="true" className={className}>
        {framingAttribution}
      </div>
    ) : null;
  }

  return (
    <div data-measure-notes="true" className={className}>
      {note && (
        <div>
          注：
          <InlineMarkdown
            text={note}
            citationMap={citationMap}
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
            citationMap={citationMap}
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
  citationMap: Record<string, CitationMapEntry>;
  framing?: BlockFraming;
  tableLayout?: TableLayout;
  chartTables?: ChartDataTable[];
  chartRequirements?: ChartRequirements;
  bibliographyById: Map<string, BibliographyEntry>;
  commentCount: number;
  commentOutline?: boolean;
  measurementOnly?: boolean;
  onCitationClick: (sourceId: string) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
}

export const ReportBlockItem = memo(function ReportBlockItem({
  slice,
  isPaged,
  printMode,
  showChartTools = true,
  citationMap,
  framing,
  tableLayout,
  chartTables,
  chartRequirements,
  bibliographyById,
  commentCount,
  commentOutline = false,
  measurementOnly = false,
  onCitationClick,
  onOpenCommentForBlock,
}: ReportBlockItemProps) {
  const { block } = slice;
  const domId = measurementOnly ? undefined : `block-${block.id}`;

  const renderInline = (text: string) => (
    <InlineMarkdown
      text={text}
      citationMap={citationMap}
      bibliographyById={bibliographyById}
      printMode={printMode}
      onCitationClick={onCitationClick}
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
    const isCallout = block.style?.variant === "callout";
    bodyNode = isCallout ? (
      <div
        id={domId}
        data-block-variant="callout"
        style={{
          fontSize: "var(--rpt-body-size, 14.5px)",
          lineHeight: "var(--rpt-line-height, 1.82)",
          borderRadius: "var(--rpt-radius, 6px)",
        }}
        className="rpt-callout my-4 rounded-lg border border-[var(--rpt-border)] border-l-[3px] border-l-[var(--rpt-accent)] bg-[var(--rpt-surface)] px-4 py-3 tracking-[0.01em] text-[var(--rpt-ink)]"
      >
        {renderInline(block.text)}
      </div>
    ) : (
      <p
        id={domId}
        style={{
          fontSize: "var(--rpt-body-size, 14.5px)",
          lineHeight: "var(--rpt-line-height, 1.82)",
          marginTop: "var(--rpt-para-gap, 0.75rem)",
          marginBottom: "var(--rpt-para-gap, 0.75rem)",
          textIndent: "var(--rpt-para-indent, 0em)",
        }}
        className="rpt-paragraph my-3 tracking-[0.01em] text-[var(--rpt-ink)] [text-align:var(--rpt-para-align,left)]"
      >
        {renderInline(block.text)}
      </p>
    );
  } else if (block.type === "quote") {
    bodyNode = (
      <blockquote
        id={domId}
        style={{
          borderRadius: "var(--rpt-radius, 6px)",
        }}
        className="rpt-quote my-4 rounded-r-lg border-l-[3px] border-[var(--rpt-accent)] bg-[var(--rpt-quote-bg,var(--rpt-surface))] px-4 py-3 text-[14px] leading-[1.75] text-[var(--rpt-ink)]/90"
      >
        {renderInline(block.text)}
      </blockquote>
    );
  } else if (block.type === "table") {
    if (slice.notesOnly) {
      bodyNode = (
        <BlockFooterNotes
          block={block}
          citationMap={citationMap}
          bibliographyById={bibliographyById}
          printMode={printMode}
          onCitationClick={onCitationClick}
          align="left"
          framingAttribution={framing?.attribution}
        />
      );
    } else {
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
            <figcaption className="text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-[var(--rpt-ink)]">
              {captionText}
              {slice.continuation ? "（续）" : ""}
            </figcaption>
          )}
          <TableBlockView
            table={block}
            layout={tableLayout}
            rowStart={slice.rowStart}
            rowEnd={slice.rowEnd}
            includeSummary={slice.includeSummary}
            containerWidthPx={isPaged ? A4_CONTENT_WIDTH_PX : undefined}
          />
          {slice.continuation &&
            slice.rowStart !== undefined &&
            slice.rowEnd !== undefined && (
              <div className="text-right text-[11px] font-normal text-[var(--rpt-muted)] tabular-nums">
                接上页 · 第 {slice.rowStart + 1}–{slice.rowEnd} 行 / 共{" "}
                {totalRows} 行
              </div>
            )}
          {slice.includeNotes !== false && slice.includeSummary !== false && (
            <BlockFooterNotes
              block={block}
              citationMap={citationMap}
              bibliographyById={bibliographyById}
              printMode={printMode}
              onCitationClick={onCitationClick}
              align="left"
              framingAttribution={framing?.attribution}
            />
          )}
        </figure>
      );
    }
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
          chartTables={chartTables}
          requirements={chartRequirements}
          renderer={isPaged || printMode ? "svg" : "canvas"}
          printSafe={isPaged || printMode}
          showTools={showChartTools}
        />
        {captionText && (
          <figcaption className="pt-1 text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-[var(--rpt-ink)]">
            {captionText}
          </figcaption>
        )}
        <BlockFooterNotes
          block={block}
          citationMap={citationMap}
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
          <figcaption className="pt-1 text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-[var(--rpt-ink)]">
            {captionText}
          </figcaption>
        )}
        <BlockFooterNotes
          block={block}
          citationMap={citationMap}
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
      data-block-kind={block.type}
      className={`group/block relative rounded-lg transition-colors hover:bg-black/[0.015] ${
        commentOutline ? "quire-comment-scope" : ""
      }`}
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

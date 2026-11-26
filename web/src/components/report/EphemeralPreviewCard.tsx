import { memo, type ReactNode } from "react";
import {
  AgentAvatar,
  resolveAgentIdentity,
} from "@/components/chat/agentAvatar";
import { InlineMarkdown } from "@/components/report/InlineMarkdown";
import {
  ReportFigureCaption,
  ReportTextBlockView,
} from "@/components/report/ReportBlockPrimitives";
import { TableBlockView } from "@/components/report/TableBlockView";
import { useSmoothStreamText } from "@/hooks/useSmoothStreamText";
import type {
  BibliographyEntry,
  BlockFraming,
  CitationMapEntry,
  ColumnDataType,
  EphemeralPreviewEvent,
  TableBlock,
  TableLayout,
} from "@/contracts/report-protocol";

function blockTypeLabel(
  blockType: string,
  level: number | null | undefined,
): string {
  switch (blockType) {
    case "heading":
      return level ? `H${level} 标题` : "章节标题";
    case "paragraph":
      return "正文段落";
    case "quote":
      return "引用块";
    case "table":
      return "数据表格";
    case "echarts":
      return "ECharts 图表";
    case "custom_html":
      return "自定义版式";
    default:
      return "报告块";
  }
}

interface ParsedTablePreview {
  headers: string[];
  rows: string[][];
  footerNote: string | null;
}

function parseStreamingTableText(
  text: string | null | undefined,
): ParsedTablePreview | null {
  if (!text || !text.includes(" | ")) {
    return null;
  }
  const rawLines = text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (rawLines.length === 0) {
    return null;
  }
  const firstLine = rawLines[0];
  if (!firstLine) {
    return null;
  }
  const headers = firstLine.split(" | ").map((cell) => cell.trim());
  const rows: string[][] = [];
  let footerNote: string | null = null;
  for (let i = 1; i < rawLines.length; i += 1) {
    const line = rawLines[i];
    if (!line) continue;
    if (line.startsWith("…") && !line.includes(" | ")) {
      footerNote = line;
      continue;
    }
    rows.push(line.split(" | ").map((cell) => cell.trim()));
  }
  return { headers, rows, footerNote };
}

interface PreviewTableModel {
  table: TableBlock;
  layout: TableLayout;
}

const PREVIEW_NUMERIC_CELL =
  /^[+-]?\d[\d,，]*(?:\.\d+)?(?:%|亿元|万元|元|项|所|个|人|篇|年|次|门|家|倍|分|名)?$/;
const PREVIEW_MISSING_CELL = /^[—–-]$/;

function previewColumnDataType(
  header: string,
  rows: string[][],
  columnIndex: number,
): ColumnDataType {
  const values = rows
    .map((row) => row[columnIndex]?.trim() ?? "")
    .filter((value) => value && !PREVIEW_MISSING_CELL.test(value));
  if (
    values.length === 0 ||
    !values.every((value) => PREVIEW_NUMERIC_CELL.test(value))
  ) {
    return "text";
  }
  if (header.includes("%") || values.every((value) => value.endsWith("%"))) {
    return "percent";
  }
  if (values.some((value) => /(?:亿元|万元|元)$/.test(value))) {
    return "currency";
  }
  return "number";
}

function committedLayoutMatches(
  layout: TableLayout | undefined,
  headers: string[],
): layout is TableLayout {
  return Boolean(
    layout &&
    layout.leaf_columns.length === headers.length &&
    layout.leaf_columns.every(
      (column, index) => column.label === headers[index],
    ),
  );
}

function buildPreviewTableModel(
  parsed: ParsedTablePreview,
  blockId: string,
  committedLayout?: TableLayout,
): PreviewTableModel {
  const table: TableBlock = {
    type: "table",
    headers: parsed.headers,
    rows: parsed.rows,
  };
  if (committedLayoutMatches(committedLayout, parsed.headers)) {
    return { table, layout: committedLayout };
  }

  const widthPct = 100 / parsed.headers.length;
  const leafColumns = parsed.headers.map((label, index) => {
    const dataType = previewColumnDataType(label, parsed.rows, index);
    const align = dataType === "text" ? ("left" as const) : ("right" as const);
    return {
      key: `column_${index}`,
      label,
      path: [label],
      data_type: dataType,
      sizing: "grow" as const,
      weight: 1,
      align,
      header_align: align,
      index,
      width_pct: widthPct,
      width_css: `${widthPct}%`,
    };
  });

  return {
    table,
    layout: {
      table_block_id: blockId,
      header_rows: [
        leafColumns.map((column) => ({
          key: column.key,
          label: column.label,
          colspan: 1,
          rowspan: 1,
          data_type: column.data_type,
          align: column.header_align,
          is_group: false,
        })),
      ],
      leaf_columns: leafColumns,
      header_depth: 1,
      total_width_pct: 100,
    },
  };
}

function StreamingCursor() {
  return (
    <span
      aria-hidden="true"
      className="ml-0.5 inline-block h-[1em] w-[1.5px] translate-y-[0.12em] animate-pulse rounded-full bg-[var(--rpt-accent,#1E3A5F)]/80"
    />
  );
}

const ReportAuthorMarginBadge = memo(function ReportAuthorMarginBadge({
  actor,
  blockType,
  level,
}: {
  actor: string;
  blockType: string;
  level: number | null | undefined;
}) {
  const identity = resolveAgentIdentity(actor);
  const typeText = blockTypeLabel(blockType, level);

  return (
    <span
      data-slot="report-author-badge"
      aria-label={`${identity.label} 正在编写${typeText}`}
      className="inline-flex size-6 items-center justify-center rounded-full bg-background/95 shadow-xs ring-1 ring-border/70"
    >
      <AgentAvatar actor={actor} size="xs" active />
    </span>
  );
});

export interface EphemeralPreviewCardProps {
  preview: EphemeralPreviewEvent;
  citationMap?: Record<string, CitationMapEntry>;
  bibliographyById?: Map<string, BibliographyEntry>;
  onCitationClick?: (sourceId: string) => void;
  framing?: BlockFraming;
  tableLayout?: TableLayout;
  /** Only the first block in a consecutive prose run shows the author badge. */
  showAuthor?: boolean;
}

/**
 * WYSIWYG streaming block renderer: renders the exact report block element
 * (heading, paragraph, quote, table, figure) with matching typography and
 * inline Markdown so the streaming tail transitions seamlessly into the
 * committed `ReportBlockItem`, while placing the active author's avatar in
 * the left margin with hover details.
 */
export function EphemeralPreviewCard({
  preview,
  citationMap,
  bibliographyById,
  onCitationClick,
  framing,
  tableLayout,
  showAuthor = true,
}: EphemeralPreviewCardProps) {
  const { block } = preview;
  const { text: smoothedText } = useSmoothStreamText(block.text ?? "", true);

  const renderInline = (text: string) => (
    <InlineMarkdown
      text={text}
      citationMap={citationMap}
      bibliographyById={bibliographyById}
      onCitationClick={onCitationClick}
    />
  );

  let body: ReactNode = null;
  if (block.block_type === "heading" || block.block_type === "quote") {
    body = (
      <ReportTextBlockView
        type={block.block_type}
        level={block.block_type === "heading" ? (block.level ?? 2) : undefined}
      >
        {renderInline(smoothedText)}
        <StreamingCursor />
      </ReportTextBlockView>
    );
  } else if (block.block_type === "table") {
    const parsedTable = parseStreamingTableText(block.text);
    const titleText = block.title?.trim();
    const captionText = [framing?.label ?? "", titleText]
      .filter((part): part is string => Boolean(part))
      .join("  ");
    const previewTable = parsedTable
      ? buildPreviewTableModel(
          parsedTable,
          block.block_id ?? preview.preview_id,
          tableLayout,
        )
      : null;
    if (previewTable && parsedTable) {
      body = (
        <figure className="my-5 space-y-2">
          {captionText && (
            <ReportFigureCaption>{captionText}</ReportFigureCaption>
          )}
          <TableBlockView
            table={previewTable.table}
            layout={previewTable.layout}
          />
          {parsedTable.footerNote && (
            <div className="pt-0.5 text-[11.5px] text-[var(--rpt-muted)]">
              {parsedTable.footerNote}
            </div>
          )}
        </figure>
      );
    } else {
      body = (
        <figure className="my-5 space-y-2">
          {captionText && (
            <ReportFigureCaption>
              {captionText}
              <StreamingCursor />
            </ReportFigureCaption>
          )}
          {block.text ? (
            <p className="my-2 text-[13.5px] leading-[1.7] text-[var(--rpt-muted)]">
              {renderInline(smoothedText)}
              <StreamingCursor />
            </p>
          ) : (
            <div className="h-24 rounded border border-dashed border-[var(--rpt-border)] bg-[var(--rpt-surface)]/50" />
          )}
        </figure>
      );
    }
  } else if (
    block.block_type === "echarts" ||
    block.block_type === "custom_html"
  ) {
    const titleText = block.title?.trim();
    body = (
      <figure className="my-5 space-y-2">
        <div className="flex min-h-[220px] flex-col items-center justify-center rounded-lg border border-dashed border-[var(--rpt-border)] bg-[var(--rpt-surface)]/40 px-6 py-8 text-center">
          <div className="flex items-end gap-2.5 opacity-45">
            <div className="h-8 w-4 rounded-t bg-[var(--rpt-muted)]/45" />
            <div className="h-14 w-4 rounded-t bg-[var(--rpt-muted)]/70" />
            <div className="h-11 w-4 rounded-t bg-[var(--rpt-muted)]/45" />
            <div className="h-16 w-4 rounded-t bg-[var(--rpt-muted)]/70" />
            <div className="h-10 w-4 rounded-t bg-[var(--rpt-muted)]/45" />
          </div>
          {block.text && (
            <p className="mt-4 max-w-md text-[12.5px] leading-relaxed text-[var(--rpt-muted)]">
              {renderInline(smoothedText)}
              <StreamingCursor />
            </p>
          )}
        </div>
        {titleText && (
          <ReportFigureCaption separated>{titleText}</ReportFigureCaption>
        )}
      </figure>
    );
  } else {
    body = (
      <ReportTextBlockView type="paragraph">
        {renderInline(smoothedText)}
        <StreamingCursor />
      </ReportTextBlockView>
    );
  }

  return (
    <div
      data-preview-id={preview.preview_id}
      data-block-id={block.block_id ?? undefined}
      data-block-kind={block.block_type}
      className="group/preview relative"
    >
      {showAuthor && (
        <>
          <div className="pointer-events-auto absolute top-0.5 right-full z-10 mr-2.5 hidden sm:flex print:hidden">
            <ReportAuthorMarginBadge
              actor={preview.actor}
              blockType={block.block_type}
              level={block.level}
            />
          </div>
          <div className="mb-1 flex items-center gap-1.5 sm:hidden print:hidden">
            <ReportAuthorMarginBadge
              actor={preview.actor}
              blockType={block.block_type}
              level={block.level}
            />
          </div>
        </>
      )}
      {body}
    </div>
  );
}

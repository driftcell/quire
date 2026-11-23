import { memo, type ReactNode } from "react";
import {
  AgentAvatar,
  resolveAgentIdentity,
} from "@/components/chat/agentAvatar";
import { InlineMarkdown } from "@/components/report/InlineMarkdown";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "@/components/ui/hover-card";
import { useSmoothStreamText } from "@/hooks/useSmoothStreamText";
import type {
  BibliographyEntry,
  CitationMapEntry,
  EphemeralPreviewEvent,
  PreviewAnchor,
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

function anchorSummaryLabel(anchor: PreviewAnchor): string | null {
  switch (anchor.type) {
    case "replace_block":
      return anchor.block_id !== "pending" ? `更新 #${anchor.block_id}` : null;
    case "after_block":
      return `插入于 #${anchor.block_id} 之后`;
    case "before_block":
      return `插入于 #${anchor.block_id} 之前`;
    case "append_to_section":
      return `追加至章节 #${anchor.section_id}`;
    case "append_to_doc":
      return "追加至报告末尾";
    default:
      return null;
  }
}

function renderHeadingTag(
  level: number,
  content: ReactNode,
) {
  switch (level) {
    case 1:
      return (
        <h1 className="mt-8 mb-4 border-b-2 border-[var(--rpt-accent)] pb-2 text-xl font-bold tracking-tight text-[var(--rpt-ink)]">
          {content}
        </h1>
      );
    case 2:
      return (
        <h2 className="mt-7 mb-3 border-b border-[var(--rpt-border)] border-l-[3px] border-l-[var(--rpt-accent)] pb-1.5 pl-2.5 text-[17px] font-semibold text-[var(--rpt-ink)]">
          {content}
        </h2>
      );
    case 3:
      return (
        <h3 className="mt-5 mb-2 text-[15px] font-semibold text-[var(--rpt-ink)]">
          {content}
        </h3>
      );
    case 4:
      return (
        <h4 className="mt-4 mb-1.5 text-[14px] font-semibold text-[var(--rpt-ink)]">
          {content}
        </h4>
      );
    default:
      return (
        <h5 className="mt-3 mb-1 text-[13px] font-semibold text-[var(--rpt-ink)]">
          {content}
        </h5>
      );
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
  blockId,
  anchor,
}: {
  actor: string;
  blockType: string;
  level: number | null | undefined;
  blockId?: string | null;
  anchor: PreviewAnchor;
}) {
  const identity = resolveAgentIdentity(actor);
  const typeText = blockTypeLabel(blockType, level);
  const anchorText = anchorSummaryLabel(anchor);
  const cleanBlockId =
    blockId && blockId !== "pending" && !blockId.startsWith("preview")
      ? blockId
      : null;

  return (
    <HoverCard>
      <HoverCardTrigger
        render={
          <button
            type="button"
            aria-label={`${identity.label} 正在编写${typeText}`}
            className="inline-flex size-6 items-center justify-center rounded-full bg-background/95 shadow-xs ring-1 ring-border/70 transition-transform hover:scale-105 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          />
        }
      >
        <AgentAvatar actor={actor} size="xs" active />
      </HoverCardTrigger>
      <HoverCardContent side="left" align="start" sideOffset={8} className="w-60 p-3">
        <div className="flex items-start gap-2.5">
          <AgentAvatar actor={actor} size="md" active />
          <div className="min-w-0 flex-1 space-y-1">
            <div className="flex items-center justify-between gap-2">
              <span className="truncate text-xs font-semibold text-foreground">
                {identity.label}
              </span>
              <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                <span className="size-1.5 animate-pulse rounded-full bg-emerald-500" />
                编写中
              </span>
            </div>
            <div className="text-[11px] text-muted-foreground">
              {identity.role}
            </div>
            <div className="flex flex-wrap items-center gap-1.5 pt-1 text-[10.5px] text-muted-foreground">
              <span className="rounded bg-muted px-1.5 py-0.5 font-medium text-foreground/80">
                {typeText}
              </span>
              {cleanBlockId && (
                <span className="font-mono text-[10px] text-muted-foreground/80">
                  #{cleanBlockId}
                </span>
              )}
            </div>
            {anchorText && (
              <div className="text-[10.5px] text-muted-foreground/80">
                {anchorText}
              </div>
            )}
          </div>
        </div>
      </HoverCardContent>
    </HoverCard>
  );
});

export interface EphemeralPreviewCardProps {
  preview: EphemeralPreviewEvent;
  citationMap?: Record<string, CitationMapEntry>;
  bibliographyById?: Map<string, BibliographyEntry>;
  onCitationClick?: (sourceId: string) => void;
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
  if (block.block_type === "heading") {
    body = renderHeadingTag(
      block.level ?? 2,
      <>
        {renderInline(smoothedText)}
        <StreamingCursor />
      </>,
    );
  } else if (block.block_type === "quote") {
    body = (
      <blockquote className="my-4 rounded-r-lg border-l-[3px] border-[var(--rpt-accent)] bg-slate-50/90 px-4 py-3 text-[14px] leading-[1.75] text-slate-700">
        {renderInline(smoothedText)}
        <StreamingCursor />
      </blockquote>
    );
  } else if (block.block_type === "table") {
    const parsedTable = parseStreamingTableText(block.text);
    const titleText = block.title?.trim();
    if (parsedTable && parsedTable.headers.length > 0) {
      body = (
        <figure className="my-5 space-y-2">
          {titleText && (
            <figcaption className="text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800">
              {titleText}
            </figcaption>
          )}
          <div className="overflow-x-auto">
            <table className="w-full border-collapse border-y-[1.5px] border-slate-800 text-left text-[12.5px]">
              <thead>
                <tr className="border-b border-slate-300 bg-slate-50/80 text-slate-700">
                  {parsedTable.headers.map((header, idx) => (
                    <th
                      key={`${header}-${idx}`}
                      className="px-2.5 py-2 font-semibold whitespace-nowrap"
                    >
                      {header}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-200/80">
                {parsedTable.rows.map((cells, rIdx) => (
                  <tr key={`row-${rIdx}`}>
                    {parsedTable.headers.map((_, cIdx) => (
                      <td
                        key={`cell-${rIdx}-${cIdx}`}
                        className="px-2.5 py-1.5 text-slate-700"
                      >
                        {cells[cIdx] ?? ""}
                        {rIdx === parsedTable.rows.length - 1 &&
                          cIdx === parsedTable.headers.length - 1 && (
                            <StreamingCursor />
                          )}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {parsedTable.footerNote && (
            <div className="pt-0.5 text-[11.5px] text-slate-500">
              {parsedTable.footerNote}
            </div>
          )}
        </figure>
      );
    } else {
      body = (
        <figure className="my-5 space-y-2">
          {titleText && (
            <figcaption className="text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800">
              {titleText}
              <StreamingCursor />
            </figcaption>
          )}
          {block.text ? (
            <p className="my-2 text-[13.5px] leading-[1.7] text-slate-600">
              {renderInline(smoothedText)}
              <StreamingCursor />
            </p>
          ) : (
            <div className="h-24 rounded border border-dashed border-slate-200 bg-slate-50/50" />
          )}
        </figure>
      );
    }
  } else if (block.block_type === "echarts" || block.block_type === "custom_html") {
    const titleText = block.title?.trim();
    body = (
      <figure className="my-5 space-y-2">
        <div className="flex min-h-[220px] flex-col items-center justify-center rounded-lg border border-dashed border-slate-200 bg-slate-50/40 px-6 py-8 text-center">
          <div className="flex items-end gap-2.5 opacity-45">
            <div className="h-8 w-4 rounded-t bg-slate-300" />
            <div className="h-14 w-4 rounded-t bg-slate-400" />
            <div className="h-11 w-4 rounded-t bg-slate-300" />
            <div className="h-16 w-4 rounded-t bg-slate-400" />
            <div className="h-10 w-4 rounded-t bg-slate-300" />
          </div>
          {block.text && (
            <p className="mt-4 max-w-md text-[12.5px] leading-relaxed text-slate-600">
              {renderInline(smoothedText)}
              <StreamingCursor />
            </p>
          )}
        </div>
        {titleText && (
          <figcaption className="pt-1 text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-slate-800">
            {titleText}
          </figcaption>
        )}
      </figure>
    );
  } else {
    body = (
      <p className="my-3 text-[14.5px] leading-[1.82] tracking-[0.01em] text-[var(--rpt-ink)]">
        {renderInline(smoothedText)}
        <StreamingCursor />
      </p>
    );
  }

  return (
    <div
      data-preview-id={preview.preview_id}
      data-block-id={block.block_id ?? undefined}
      data-block-kind={block.block_type}
      className="group/preview relative"
    >
      <div className="pointer-events-auto absolute top-0.5 right-full z-10 mr-2.5 hidden sm:flex print:hidden">
        <ReportAuthorMarginBadge
          actor={preview.actor}
          blockType={block.block_type}
          level={block.level}
          blockId={block.block_id}
          anchor={preview.anchor}
        />
      </div>
      <div className="mb-1 flex items-center gap-1.5 sm:hidden print:hidden">
        <ReportAuthorMarginBadge
          actor={preview.actor}
          blockType={block.block_type}
          level={block.level}
          blockId={block.block_id}
          anchor={preview.anchor}
        />
      </div>
      {body}
    </div>
  );
}

import { useSmoothStreamText } from "@/hooks/useSmoothStreamText";
import type {
  EphemeralPreviewEvent,
  PreviewAnchor,
} from "@/contracts/report-protocol";

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

function blockTypeBadgeLabel(
  blockType: string,
  level: number | null | undefined,
): string {
  switch (blockType) {
    case "heading":
      return level ? `章节标题 H${level}` : "章节标题";
    case "paragraph":
      return "正文撰写";
    case "quote":
      return "核心引述";
    case "table":
      return "数据表格";
    case "echarts":
      return "图表可视化";
    case "custom_html":
      return "定制版式";
    default:
      return "内容块";
  }
}

function anchorBadgeLabel(anchor: PreviewAnchor): string | null {
  switch (anchor.type) {
    case "replace_block":
      return anchor.block_id !== "pending" ? `修订 #${anchor.block_id}` : null;
    case "after_block":
      return `插入于 #${anchor.block_id} 后`;
    case "before_block":
      return `插入于 #${anchor.block_id} 前`;
    case "append_to_section":
      return `追加至章节 #${anchor.section_id}`;
    case "append_to_doc":
      return "追加至文末";
    default:
      return null;
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

export function EphemeralPreviewCard({
  preview,
}: {
  preview: EphemeralPreviewEvent;
}) {
  const { block } = preview;
  const { text: smoothedText } = useSmoothStreamText(block.text ?? "", true);
  const cursor = (
    <span className="ml-0.5 inline-block h-[1.05em] w-[2px] animate-pulse bg-[var(--rpt-accent,#1E3A5F)]/75 align-text-bottom" />
  );
  const paragraphClass =
    "m-0 whitespace-pre-wrap text-[14.5px] leading-[1.82] text-[var(--rpt-ink,#1F2937)]";

  let body: React.ReactNode;
  if (block.block_type === "heading") {
    body = (
      <div
        className={`font-[family-name:var(--rpt-font-heading)] text-[var(--rpt-ink,#1F2937)] ${headingPreviewClass(block.level)}`}
      >
        {smoothedText}
        {cursor}
      </div>
    );
  } else if (block.block_type === "quote") {
    body = (
      <blockquote className="m-0 rounded-r-md border-l-2 border-[var(--rpt-accent,#1E3A5F)]/50 bg-slate-50/90 px-4 py-3 text-[14px] leading-[1.75] text-slate-700">
        {smoothedText}
        {cursor}
      </blockquote>
    );
  } else if (block.block_type === "table") {
    const parsedTable = parseStreamingTableText(block.text);
    if (parsedTable && parsedTable.headers.length > 0) {
      body = (
        <div className="overflow-hidden rounded-lg border border-slate-200/90 bg-white/90 shadow-2xs">
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-left text-[12.5px]">
              <thead>
                <tr className="border-b border-slate-200/80 bg-slate-50/90 text-slate-600">
                  {parsedTable.headers.map((header, idx) => (
                    <th
                      key={`${header}-${idx}`}
                      className="px-3 py-2 font-semibold whitespace-nowrap"
                    >
                      {header}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {parsedTable.rows.map((cells, rIdx) => (
                  <tr key={`row-${rIdx}`} className="bg-white/70">
                    {parsedTable.headers.map((_, cIdx) => (
                      <td
                        key={`cell-${rIdx}-${cIdx}`}
                        className="px-3 py-1.5 text-slate-700"
                      >
                        {cells[cIdx] ?? ""}
                        {rIdx === parsedTable.rows.length - 1 &&
                          cIdx === parsedTable.headers.length - 1 &&
                          cursor}
                      </td>
                    ))}
                  </tr>
                ))}
                <tr className="bg-slate-50/40" aria-hidden="true">
                  {parsedTable.headers.map((_, idx) => (
                    <td key={`skel-${idx}`} className="px-3 py-2">
                      <span className="sr-only">加载中</span>
                      <div className="h-2.5 w-3/4 animate-pulse rounded bg-slate-200/70" />
                    </td>
                  ))}
                </tr>
              </tbody>
            </table>
          </div>
          {parsedTable.footerNote && (
            <div className="border-t border-slate-100 bg-slate-50/70 px-3 py-1.5 text-[11px] text-slate-500">
              {parsedTable.footerNote}
            </div>
          )}
        </div>
      );
    } else if (block.text) {
      body = (
        <p className={paragraphClass}>
          {smoothedText}
          {cursor}
        </p>
      );
    } else {
      body = (
        <div className="space-y-1.5 rounded-lg border border-slate-200/80 bg-slate-100/80 p-2.5">
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
    }
  } else if (block.block_type === "echarts") {
    body = (
      <div className="space-y-2 rounded-lg border border-slate-200/80 bg-slate-50/80 p-3.5">
        <div className="flex h-36 items-end justify-around gap-3 px-2 pt-2">
          <div className="h-1/2 w-8 animate-pulse rounded-t bg-slate-200/80" />
          <div className="h-4/5 w-8 animate-pulse rounded-t bg-slate-300/70" />
          <div className="h-2/3 w-8 animate-pulse rounded-t bg-slate-200/70" />
          <div className="h-full w-8 animate-pulse rounded-t bg-slate-300/60" />
          <div className="h-3/5 w-8 animate-pulse rounded-t bg-slate-200/60" />
        </div>
        {block.text && (
          <div className="border-t border-slate-200/60 pt-2 text-[12px] leading-relaxed text-slate-600">
            {smoothedText}
            {cursor}
          </div>
        )}
      </div>
    );
  } else if (block.text) {
    body = (
      <p className={paragraphClass}>
        {smoothedText}
        {cursor}
      </p>
    );
  } else {
    body = <div className="h-20 animate-pulse rounded-lg bg-slate-100/80" />;
  }

  const actorLabel = preview.actor === "main" ? "主笔 Agent" : preview.actor;
  const typeLabel = blockTypeBadgeLabel(block.block_type, block.level);
  const anchorLabel = anchorBadgeLabel(preview.anchor);
  const showBlockId =
    block.block_id &&
    block.block_id !== "pending" &&
    !block.block_id.startsWith("preview");

  return (
    <div
      data-preview-id={preview.preview_id}
      data-block-id={block.block_id ?? undefined}
      className="my-3 rounded-r-lg border-l-2 border-[var(--rpt-accent,#1E3A5F)]/55 bg-slate-50/45 py-2.5 pr-3 pl-3.5 transition-all duration-200"
    >
      <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2 text-[11px] text-slate-500">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="inline-flex items-center gap-1.5 font-medium text-[var(--rpt-accent,#1E3A5F)]">
            <span className="size-1.5 animate-pulse rounded-full bg-[var(--rpt-accent,#1E3A5F)]" />
            {actorLabel}
          </span>
          <span className="rounded bg-slate-200/70 px-1.5 py-0.5 text-[10px] font-medium text-slate-600">
            {typeLabel}
          </span>
          {anchorLabel && (
            <span className="text-[10.5px] text-slate-400">{anchorLabel}</span>
          )}
        </div>
        {showBlockId && (
          <span className="font-mono text-[10px] text-slate-400">
            #{block.block_id}
          </span>
        )}
      </div>
      {block.title && (
        <div className="mb-1.5 font-[family-name:var(--rpt-font-heading)] text-[14.5px] font-semibold text-[var(--rpt-ink,#1F2937)]">
          {block.title}
        </div>
      )}
      {body}
    </div>
  );
}

import { useSmoothStreamText } from "@/hooks/useSmoothStreamText";
import type { EphemeralPreviewEvent } from "@/contracts/report-protocol";

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

export function EphemeralPreviewCard({
  preview,
}: {
  preview: EphemeralPreviewEvent;
}) {
  const { block } = preview;
  const { text: smoothedText } = useSmoothStreamText(block.text ?? "", true);
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
        {smoothedText}
        {cursor}
      </div>
    );
  } else if (block.block_type === "quote") {
    body = (
      <blockquote className="m-0 whitespace-pre-wrap bg-slate-50/90 px-4 py-3 text-[14px] leading-[1.75] text-slate-700">
        {smoothedText}
        {cursor}
      </blockquote>
    );
  } else if (block.block_type === "table") {
    body = block.text ? (
      <p className={paragraphClass}>
        {smoothedText}
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
        {smoothedText}
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

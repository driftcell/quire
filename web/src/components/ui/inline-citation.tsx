import { useState } from "react";
import { ExternalLinkIcon, QuoteIcon } from "lucide-react";
import { cn } from "cn";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "@/components/ui/hover-card";

export interface InlineCitationSource {
  id: string;
  index?: number;
  title: string;
  url?: string | null;
  author?: string | null;
  publishedAt?: string | null;
  publisher?: string | null;
  excerpt?: string | null;
  note?: string | null;
}

export interface InlineCitationProps {
  sourceId: string;
  source?: InlineCitationSource;
  index?: number;
  className?: string;
}

/**
 * shadcn/ui AI Elements Inline Citation component built on HoverCard + Badge button.
 * Renders a compact source pill inline in prose and reveals the source card on hover or click.
 */
export function InlineCitation({
  sourceId,
  source,
  index,
  className,
}: InlineCitationProps) {
  const [open, setOpen] = useState(false);
  const displayIndex = index ?? source?.index;
  const rawTitle = source?.title?.trim() || sourceId;
  const shortLabel =
    source?.publisher?.trim() ||
    (rawTitle.length > 14 ? `${rawTitle.slice(0, 14)}…` : rawTitle);

  return (
    <HoverCard open={open} onOpenChange={setOpen}>
      <HoverCardTrigger
        delay={60}
        closeDelay={120}
        render={
          <button
            type="button"
            data-slot="inline-citation"
            data-source-id={sourceId}
            aria-label={`引用来源 ${rawTitle}`}
            onClick={() => {
              setOpen((prev) => !prev);
            }}
            className={cn(
              "mx-0.5 inline-flex h-5 max-w-[190px] translate-y-[-1px] cursor-pointer items-center gap-1 rounded-full border border-primary/25 bg-primary/8 px-2 py-0 text-[11px] leading-none font-medium text-primary align-baseline transition-colors hover:border-primary/45 hover:bg-primary/15 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
              className,
            )}
          />
        }
      >
        {displayIndex !== undefined && (
          <span className="font-mono text-[10px] font-semibold tabular-nums">
            [{displayIndex}]
          </span>
        )}
        <span className="truncate">{shortLabel}</span>
      </HoverCardTrigger>

      <HoverCardContent
        side="top"
        align="start"
        className="w-80 space-y-2 rounded-xl border border-border/80 bg-popover p-3 text-xs shadow-lg"
      >
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              {displayIndex !== undefined && (
                <span className="inline-flex size-4 shrink-0 items-center justify-center rounded bg-primary/10 font-mono text-[10px] font-semibold text-primary">
                  {displayIndex}
                </span>
              )}
              <span className="font-mono text-[10px] text-muted-foreground">
                {sourceId}
              </span>
            </div>
            <div className="mt-1 line-clamp-2 font-semibold text-foreground">
              {rawTitle}
            </div>
          </div>
          {source?.url && (
            <a
              href={source.url}
              target="_blank"
              rel="noreferrer noopener"
              aria-label="打开原始链接"
              className="inline-flex size-6 shrink-0 items-center justify-center rounded-md border border-border/60 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <ExternalLinkIcon className="size-3.5" />
            </a>
          )}
        </div>

        {(source?.publisher || source?.author || source?.publishedAt) && (
          <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
            {source?.publisher && <span>{source.publisher}</span>}
            {source?.author && <span>· {source.author}</span>}
            {source?.publishedAt && <span>· {source.publishedAt}</span>}
          </div>
        )}

        {source?.excerpt && (
          <div className="rounded-lg border-l-2 border-primary/40 bg-muted/40 px-2.5 py-1.5 text-[11px] leading-4 text-muted-foreground">
            <div className="mb-0.5 flex items-center gap-1 text-[10px] font-medium text-foreground/75">
              <QuoteIcon className="size-2.5" />
              原文摘录
            </div>
            <p className="line-clamp-4 whitespace-pre-wrap">{source.excerpt}</p>
          </div>
        )}

        {!source && (
          <p className="text-[11px] text-muted-foreground">
            来源 ID <code className="font-mono">{sourceId}</code> 已标记引用。
          </p>
        )}
      </HoverCardContent>
    </HoverCard>
  );
}

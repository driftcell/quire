import { useState } from "react";
import { ExternalLinkIcon } from "lucide-react";
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

function extractDomainLabel(url: string | null | undefined): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url);
    return parsed.hostname.replace(/^www\./, "");
  } catch {
    return null;
  }
}

/**
 * Vercel / Perplexity-inspired Inline Citation badge + HoverCard.
 * Renders a refined, compact numeric pill inline so prose line-height and
 * copy-paste rhythm are never disrupted, and reveals the full source card on hover or click.
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
  const publisherLabel =
    source?.publisher?.trim() ||
    source?.author?.trim() ||
    extractDomainLabel(source?.url);

  const badgeText =
    displayIndex !== undefined
      ? String(displayIndex)
      : sourceId.slice(0, 6);

  return (
    <HoverCard open={open} onOpenChange={setOpen}>
      <HoverCardTrigger
        delay={50}
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
              "mx-0.5 inline-flex h-[18px] min-w-[18px] -translate-y-[1px] cursor-pointer items-center justify-center rounded-full border border-border/80 bg-foreground/[0.045] px-1.5 font-mono text-[10.5px] leading-none font-medium text-foreground/80 tabular-nums align-baseline transition-colors hover:border-foreground/50 hover:bg-foreground hover:text-background focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
              className,
            )}
          />
        }
      >
        {badgeText}
      </HoverCardTrigger>

      <HoverCardContent
        side="top"
        align="start"
        className="w-80 space-y-2.5 rounded-xl border border-border/75 bg-popover/98 p-3.5 text-xs shadow-xl backdrop-blur-md"
      >
        <div className="flex items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-1.5">
            {displayIndex !== undefined && (
              <span className="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full bg-foreground/[0.07] px-1 font-mono text-[10px] font-medium text-foreground/85 tabular-nums">
                {displayIndex}
              </span>
            )}
            <span className="truncate text-[11px] font-medium text-muted-foreground">
              {publisherLabel ?? sourceId}
            </span>
            {source?.publishedAt && (
              <span className="shrink-0 font-mono text-[10.5px] text-muted-foreground/70 tabular-nums">
                · {source.publishedAt}
              </span>
            )}
          </div>
          {source?.url && (
            <a
              href={source.url}
              target="_blank"
              rel="noreferrer noopener"
              aria-label="打开原始链接"
              className="inline-flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:text-foreground"
            >
              <ExternalLinkIcon className="size-3.5" />
            </a>
          )}
        </div>

        <div className="line-clamp-2 text-[13px] leading-snug font-medium text-foreground">
          {rawTitle}
        </div>

        {source?.excerpt && (
          <blockquote className="border-l-2 border-foreground/20 pl-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
            <p className="line-clamp-4 whitespace-pre-wrap">{source.excerpt}</p>
          </blockquote>
        )}

        {!source?.excerpt && source?.note && (
          <p className="line-clamp-3 text-[11.5px] leading-relaxed text-muted-foreground">
            {source.note}
          </p>
        )}
      </HoverCardContent>
    </HoverCard>
  );
}

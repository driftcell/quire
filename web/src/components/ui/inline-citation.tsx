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

function truncateShortLabel(text: string, maxChars = 16): string {
  const clean = text.replace(/^[《「『【]|[》」』】]$/g, "").trim();
  const chars = Array.from(clean);
  if (chars.length <= maxChars) {
    return clean;
  }
  return `${chars.slice(0, maxChars).join("")}…`;
}

/**
 * Derive a concise, human-readable short tag for an inline citation badge
 * (e.g. "浙江理工大学官网", "高等教育研究所", "青塔教育", "浙江省人民政府").
 */
export function resolveShortCitationLabel(
  source: InlineCitationSource | undefined,
  sourceId: string,
): string {
  const publisher = source?.publisher?.trim();
  if (publisher) {
    return truncateShortLabel(publisher);
  }
  const author = source?.author?.trim();
  if (author) {
    return truncateShortLabel(author);
  }
  const rawTitle = source?.title?.trim();
  if (rawTitle && rawTitle !== sourceId) {
    const prefix = rawTitle.split(/\s*(?:[—–\-｜|·:：]|——)\s*/)[0]?.trim();
    if (prefix && prefix.length >= 2) {
      return truncateShortLabel(prefix);
    }
    return truncateShortLabel(rawTitle);
  }
  const domain = extractDomainLabel(source?.url);
  if (domain) {
    return truncateShortLabel(domain, 20);
  }
  return truncateShortLabel(sourceId, 16);
}

/**
 * Inline Citation short-tag badge + HoverCard.
 * Renders a concise semantic source label inline (e.g. "浙江理工大学官网", "高等教育研究所")
 * and reveals the full source provenance card on hover or click.
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

  const badgeText = resolveShortCitationLabel(source, sourceId);

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
              "mx-0.5 inline-flex h-[19px] max-w-[14rem] -translate-y-[0.5px] cursor-pointer items-center justify-center rounded-full border border-border/80 bg-foreground/[0.045] px-2 text-[11px] leading-none font-medium text-foreground/80 align-baseline transition-colors hover:border-foreground/50 hover:bg-foreground hover:text-background focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
              className,
            )}
          />
        }
      >
        <span className="truncate">{badgeText}</span>
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

import { useState } from "react";
import { XIcon } from "lucide-react";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "@/components/ui/hover-card";

export interface CitationRef {
  citeId: string;
  sourceId: string;
  number: number | null;
  label: string;
  badge: string;
  title: string;
  authors?: string;
  date?: string | null;
  excerpt?: string | null;
  url?: string | null;
}

function sourceDomain(url: string | null | undefined): string | null {
  if (!url) {
    return null;
  }
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return null;
  }
}

export interface CitationHoverCardProps {
  refs: CitationRef[];
  label: string;
  onLocate: (sourceId: string) => void;
}

export function CitationHoverCard({
  refs,
  label,
  onLocate,
}: CitationHoverCardProps) {
  const [open, setOpen] = useState(false);
  const first = refs[0];
  if (!first) {
    return null;
  }

  return (
    <sup className="ml-0.5">
      <HoverCard open={open} onOpenChange={setOpen}>
        <HoverCardTrigger
          render={
            <button
              type="button"
              aria-label={`引用 ${refs.map((ref) => ref.citeId).join(", ")}`}
              onClick={() => {
                onLocate(first.sourceId);
                setOpen(false);
              }}
              className="rounded px-1 py-0.5 text-[11px] font-medium text-muted-foreground tabular-nums transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            />
          }
        >
          {label}
        </HoverCardTrigger>
        <HoverCardContent
          aria-label="引用来源"
          side="bottom"
          align="start"
          className="w-80 max-w-[calc(100vw-2rem)] overflow-hidden p-0"
        >
          <div className="flex items-center justify-between border-b px-3 py-2">
            <div className="text-xs font-semibold">
              引用了 {refs.length} 个来源
            </div>
            <button
              type="button"
              aria-label="关闭引用浮层"
              onClick={() => setOpen(false)}
              className="rounded p-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <XIcon className="size-3.5" />
            </button>
          </div>
          <div className="no-scrollbar max-h-72 divide-y overflow-y-auto overscroll-contain">
            {refs.map((ref) => {
              const domain = sourceDomain(ref.url);
              return (
                <div key={ref.citeId} className="flex gap-2 px-3 py-2.5">
                  <button
                    type="button"
                    title="定位到文末"
                    onClick={() => {
                      onLocate(ref.sourceId);
                      setOpen(false);
                    }}
                    className="mt-0.5 shrink-0 self-start rounded bg-muted px-1 py-0.5 font-mono text-[10px] text-muted-foreground tabular-nums transition-colors hover:bg-muted-foreground/15 hover:text-foreground"
                  >
                    {ref.badge}
                  </button>
                  <div className="min-w-0 flex-1 space-y-0.5">
                    {ref.url ? (
                      <a
                        href={ref.url}
                        target="_blank"
                        rel="noreferrer noopener"
                        className="line-clamp-2 min-w-0 text-xs font-medium text-foreground underline-offset-2 hover:underline"
                      >
                        {ref.title}
                      </a>
                    ) : (
                      <span className="line-clamp-2 min-w-0 text-xs font-medium">
                        {ref.title}
                      </span>
                    )}
                    {(domain || ref.authors || ref.date) && (
                      <div className="truncate text-[11px] text-muted-foreground">
                        {domain && (
                          <span className="text-muted-foreground hover:text-foreground">
                            {domain}
                          </span>
                        )}
                        {domain && (ref.authors || ref.date) ? " · " : ""}
                        {ref.authors}
                        {ref.authors && ref.date ? " · " : ""}
                        {ref.date}
                      </div>
                    )}
                    {ref.excerpt && (
                      <div className="mt-1 line-clamp-3 border-l-2 border-muted-foreground/30 pl-2 text-[11px] leading-relaxed text-muted-foreground italic">
                        {ref.excerpt}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </HoverCardContent>
      </HoverCard>
    </sup>
  );
}

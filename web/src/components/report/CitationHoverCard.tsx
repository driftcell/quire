import { useState } from "react";
import {
  FloatingPortal,
  autoUpdate,
  flip,
  offset,
  safePolygon,
  shift,
  useDismiss,
  useFloating,
  useFocus,
  useHover,
  useInteractions,
  useRole,
} from "@floating-ui/react";
import { BookOpenIcon, Globe2Icon, XIcon } from "lucide-react";

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
  const {
    refs: floatingRefs,
    floatingStyles,
    context,
  } = useFloating({
    open,
    onOpenChange: setOpen,
    placement: "bottom-start",
    whileElementsMounted: autoUpdate,
    middleware: [offset(8), flip({ padding: 8 }), shift({ padding: 8 })],
  });
  const hover = useHover(context, {
    move: false,
    delay: { open: 120, close: 180 },
    handleClose: safePolygon(),
  });
  const focus = useFocus(context);
  const dismiss = useDismiss(context, { escapeKey: true, outsidePress: true });
  const role = useRole(context, { role: "dialog" });
  const { getReferenceProps, getFloatingProps } = useInteractions([
    hover,
    focus,
    dismiss,
    role,
  ]);

  const first = refs[0];
  if (!first) {
    return null;
  }

  return (
    <>
      <sup className="ml-0.5">
        <button
          ref={(node) => {
            floatingRefs.setReference(node);
          }}
          type="button"
          aria-label={`引用 ${refs.map((ref) => ref.citeId).join(", ")}`}
          {...getReferenceProps({
            onClick: () => {
              onLocate(first.sourceId);
              setOpen(false);
            },
            className:
              "rounded px-0.5 text-[11px] font-medium text-blue-600 no-underline transition-colors hover:bg-blue-50 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none dark:text-blue-400 dark:hover:bg-blue-950/50",
          })}
        >
          {label}
        </button>
      </sup>
      {open && (
        <FloatingPortal>
          <div
            ref={(node) => {
              floatingRefs.setFloating(node);
            }}
            style={floatingStyles}
            aria-label="引用来源"
            {...getFloatingProps({
              className:
                "z-[60] w-[22rem] max-w-[calc(100vw-2rem)] overflow-hidden rounded-xl border bg-popover text-popover-foreground shadow-xl ring-1 ring-black/5 dark:ring-white/10",
            })}
          >
            <div className="flex items-center justify-between border-b px-3 py-2">
              <div className="flex items-center gap-1.5 text-xs font-semibold">
                <BookOpenIcon className="size-3.5 text-primary" />
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
            <div className="max-h-72 divide-y overflow-y-auto overscroll-contain">
              {refs.map((ref) => {
                const domain = sourceDomain(ref.url);
                return (
                  <div key={ref.citeId} className="flex gap-2 px-3 py-2.5">
                    <span className="mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
                      <Globe2Icon className="size-3.5" />
                    </span>
                    <div className="min-w-0 flex-1 space-y-0.5">
                      <div className="flex items-start gap-2">
                        {ref.url ? (
                          <a
                            href={ref.url}
                            target="_blank"
                            rel="noreferrer noopener"
                            className="line-clamp-2 min-w-0 text-xs font-medium text-blue-700 hover:underline dark:text-blue-300"
                          >
                            {ref.title}
                          </a>
                        ) : (
                          <span className="line-clamp-2 min-w-0 text-xs font-medium">
                            {ref.title}
                          </span>
                        )}
                        <button
                          type="button"
                          title="定位到文末"
                          onClick={() => {
                            onLocate(ref.sourceId);
                            setOpen(false);
                          }}
                          className="ml-auto shrink-0 rounded bg-muted px-1 py-0.5 font-mono text-[10px] text-muted-foreground transition-colors hover:bg-muted-foreground/15 hover:text-foreground"
                        >
                          {ref.badge}
                        </button>
                      </div>
                      {(domain || ref.authors || ref.date) && (
                        <div className="truncate text-[11px] text-muted-foreground">
                          {domain && (
                            <span className="text-blue-600 dark:text-blue-400">
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
                        <div className="mt-1 line-clamp-3 border-l-2 border-amber-500/60 pl-2 text-[11px] leading-relaxed text-muted-foreground italic">
                          {ref.excerpt}
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </FloatingPortal>
      )}
    </>
  );
}

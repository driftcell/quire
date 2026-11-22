import { useCallback, useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import type {
  CommentThread,
  TextQuoteSelector,
} from "@/contracts/report-protocol";
import type { ReportSelectionDescriptor } from "@/components/report/reportSelection";
import { commentThreadBlockIds } from "@/components/report/reportSelection";
import {
  locateQuoteRange,
  visibleBlockElements,
} from "@/components/report/selectionRange";

const COMMENT_HIGHLIGHT = "quire-comment";
const ACTIVE_HIGHLIGHT = "quire-comment-active";

interface HighlightRegistryLike {
  set(name: string, highlight: unknown): void;
  delete(name: string): void;
}

type HighlightConstructor = new (...ranges: Range[]) => unknown;

export interface HoveredCommentAnchor {
  threads: CommentThread[];
  blockId: string;
  rect: {
    top: number;
    left: number;
    bottom: number;
    right: number;
    width: number;
    height: number;
  };
}

interface ThreadRangeEntry {
  thread: CommentThread;
  blockIds: string[];
  ranges: Range[];
}

interface UseReportHighlightsOptions {
  containerRef: RefObject<HTMLElement | null>;
  comments: CommentThread[];
  activeThreadId?: string | null;
  pendingSelection?: ReportSelectionDescriptor | null;
  enableHoverPreview?: boolean;
}

function isHighlightRegistry(value: unknown): value is HighlightRegistryLike {
  if (!value || typeof value !== "object") return false;
  return (
    typeof Reflect.get(value, "set") === "function" &&
    typeof Reflect.get(value, "delete") === "function"
  );
}

function isHighlightConstructor(value: unknown): value is HighlightConstructor {
  return typeof value === "function";
}

function highlightSupport(): {
  registry: HighlightRegistryLike | null;
  HighlightCtor: HighlightConstructor | null;
} {
  const cssValue = Reflect.get(globalThis, "CSS");
  const registry =
    cssValue && typeof cssValue === "object"
      ? Reflect.get(cssValue, "highlights")
      : null;
  const HighlightCtor = Reflect.get(globalThis, "Highlight");
  return {
    registry: isHighlightRegistry(registry) ? registry : null,
    HighlightCtor: isHighlightConstructor(HighlightCtor) ? HighlightCtor : null,
  };
}

function rangesForBlock(
  container: HTMLElement,
  blockId: string,
  selector: TextQuoteSelector | null | undefined,
): Range[] {
  if (!selector) return [];
  const elements = visibleBlockElements(container).filter(
    (element) => element.dataset.blockId === blockId,
  );
  const ranges: Range[] = [];
  for (const element of elements) {
    const range = locateQuoteRange(element, selector);
    if (range) {
      ranges.push(range);
    }
  }
  return ranges;
}

function copyRect(rect: DOMRect) {
  return {
    top: rect.top,
    left: rect.left,
    bottom: rect.bottom,
    right: rect.right,
    width: rect.width,
    height: rect.height,
  };
}

export function useReportHighlights({
  containerRef,
  comments,
  activeThreadId,
  pendingSelection,
  enableHoverPreview = true,
}: UseReportHighlightsOptions) {
  const { registry, HighlightCtor } = highlightSupport();
  const supported = Boolean(registry && HighlightCtor);
  const threadRangesRef = useRef<ThreadRangeEntry[]>([]);
  const [hoveredComment, setHoveredComment] =
    useState<HoveredCommentAnchor | null>(null);
  const hoverCardHoveredRef = useRef(false);
  const hideTimerRef = useRef<number | null>(null);

  const clearHideTimer = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  }, []);

  const keepHoverOpen = useCallback(() => {
    hoverCardHoveredRef.current = true;
    clearHideTimer();
  }, [clearHideTimer]);

  const scheduleHoverClose = useCallback(() => {
    hoverCardHoveredRef.current = false;
    clearHideTimer();
    hideTimerRef.current = window.setTimeout(() => {
      if (!hoverCardHoveredRef.current) {
        setHoveredComment(null);
      }
    }, 180);
  }, [clearHideTimer]);

  const dismissHover = useCallback(() => {
    hoverCardHoveredRef.current = false;
    clearHideTimer();
    setHoveredComment(null);
  }, [clearHideTimer]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      threadRangesRef.current = [];
      return undefined;
    }
    const commentRanges: Range[] = [];
    const activeRanges: Range[] = [];
    const entries: ThreadRangeEntry[] = [];

    for (const thread of comments) {
      if (thread.anchor_detached) {
        continue;
      }
      const isActivelySelected = thread.thread_id === activeThreadId;
      // Resolved (addressed/archived) comments are hidden from the report canvas
      // unless the user explicitly clicked that thread in the comments drawer.
      if (thread.status !== "open" && !isActivelySelected) {
        continue;
      }
      const blockIds = commentThreadBlockIds(thread);
      const startBlockId = blockIds[0];
      if (!startBlockId) continue;

      const threadRanges: Range[] = [
        ...rangesForBlock(container, startBlockId, thread.quote),
      ];
      const endBlockId = thread.end_block_id?.toString();
      if (endBlockId) {
        threadRanges.push(
          ...rangesForBlock(container, endBlockId, thread.end_quote),
        );
      }

      if (thread.status === "open") {
        entries.push({
          thread,
          blockIds,
          ranges: threadRanges,
        });
      }

      const target = isActivelySelected ? activeRanges : commentRanges;
      target.push(...threadRanges);
    }

    threadRangesRef.current = entries;

    if (!registry || !HighlightCtor) {
      return undefined;
    }

    if (pendingSelection) {
      activeRanges.push(
        ...rangesForBlock(
          container,
          pendingSelection.blockId,
          pendingSelection.quote,
        ),
      );
      if (pendingSelection.endBlockId) {
        activeRanges.push(
          ...rangesForBlock(
            container,
            pendingSelection.endBlockId,
            pendingSelection.endQuote,
          ),
        );
      }
    }

    if (commentRanges.length > 0) {
      registry.set(COMMENT_HIGHLIGHT, new HighlightCtor(...commentRanges));
    } else {
      registry.delete(COMMENT_HIGHLIGHT);
    }
    if (activeRanges.length > 0) {
      registry.set(ACTIVE_HIGHLIGHT, new HighlightCtor(...activeRanges));
    } else {
      registry.delete(ACTIVE_HIGHLIGHT);
    }

    return () => {
      registry.delete(COMMENT_HIGHLIGHT);
      registry.delete(ACTIVE_HIGHLIGHT);
    };
  }, [
    HighlightCtor,
    activeThreadId,
    comments,
    containerRef,
    pendingSelection,
    registry,
  ]);

  useEffect(() => {
    if (!enableHoverPreview) {
      return undefined;
    }
    const container = containerRef.current;
    if (!container) {
      return undefined;
    }

    const onPointerMove = (event: PointerEvent) => {
      const { clientX, clientY } = event;
      const entries = threadRangesRef.current;
      if (entries.length === 0) {
        if (hoveredComment) {
          setHoveredComment(null);
        }
        return;
      }

      // 1. Check if pointer is over any inline comment underline (横线)
      const matchedInline: CommentThread[] = [];
      let matchedRect: DOMRect | null = null;
      let matchedBlockId = "";

      for (const entry of entries) {
        for (const range of entry.ranges) {
          const rects = range.getClientRects();
          for (let i = 0; i < rects.length; i += 1) {
            const rect = rects[i];
            if (!rect || rect.width === 0 || rect.height === 0) continue;
            if (
              clientX >= rect.left - 4 &&
              clientX <= rect.right + 4 &&
              clientY >= rect.top - 3 &&
              clientY <= rect.bottom + 6
            ) {
              if (
                !matchedInline.some(
                  (candidate) => candidate.thread_id === entry.thread.thread_id,
                )
              ) {
                matchedInline.push(entry.thread);
              }
              if (!matchedRect) {
                matchedRect = rect;
                matchedBlockId = entry.blockIds[0] ?? entry.thread.block_id;
              }
              break;
            }
          }
        }
      }

      if (matchedInline.length > 0 && matchedRect) {
        clearHideTimer();
        const nextRect = copyRect(matchedRect);
        setHoveredComment((prev) => {
          if (
            prev &&
            prev.blockId === matchedBlockId &&
            prev.threads.length === matchedInline.length &&
            prev.threads.every(
              (thread, idx) =>
                thread.thread_id === matchedInline[idx]?.thread_id,
            )
          ) {
            return prev;
          }
          return {
            threads: matchedInline,
            blockId: matchedBlockId,
            rect: nextRect,
          };
        });
        return;
      }

      // 2. Check if pointer is over the left vertical rule (竖线) of a .quire-comment-scope block
      const scopeElements = container.querySelectorAll<HTMLElement>(
        ".quire-comment-scope[data-block-id]",
      );
      for (let i = 0; i < scopeElements.length; i += 1) {
        const element = scopeElements[i];
        const blockId = element?.dataset.blockId;
        if (!element || !blockId) continue;
        const rect = element.getBoundingClientRect();
        if (
          clientY >= rect.top - 4 &&
          clientY <= rect.bottom + 4 &&
          clientX >= rect.left - 22 &&
          clientX <= rect.left + 26
        ) {
          const blockThreads = entries
            .filter((entry) => entry.blockIds.includes(blockId))
            .map((entry) => entry.thread);
          if (blockThreads.length > 0) {
            clearHideTimer();
            const barRect = {
              top: rect.top,
              left: rect.left - 8,
              bottom: Math.min(rect.bottom, rect.top + 48),
              right: rect.left + 8,
              width: 16,
              height: Math.min(rect.height, 48),
            };
            setHoveredComment((prev) => {
              if (
                prev &&
                prev.blockId === blockId &&
                prev.threads.length === blockThreads.length &&
                prev.threads.every(
                  (thread, idx) =>
                    thread.thread_id === blockThreads[idx]?.thread_id,
                )
              ) {
                return prev;
              }
              return {
                threads: blockThreads,
                blockId,
                rect: barRect,
              };
            });
            return;
          }
        }
      }

      if (!hoverCardHoveredRef.current && hideTimerRef.current === null) {
        hideTimerRef.current = window.setTimeout(() => {
          hideTimerRef.current = null;
          if (!hoverCardHoveredRef.current) {
            setHoveredComment(null);
          }
        }, 160);
      }
    };

    container.addEventListener("pointermove", onPointerMove, { passive: true });
    return () => {
      container.removeEventListener("pointermove", onPointerMove);
      clearHideTimer();
    };
  }, [clearHideTimer, containerRef, enableHoverPreview, hoveredComment]);

  return {
    supported,
    hoveredComment,
    keepHoverOpen,
    scheduleHoverClose,
    dismissHover,
  };
}

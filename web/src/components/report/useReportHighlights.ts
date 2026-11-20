import { useEffect } from "react";
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

interface UseReportHighlightsOptions {
  containerRef: RefObject<HTMLElement | null>;
  comments: CommentThread[];
  activeThreadId?: string | null;
  pendingSelection?: ReportSelectionDescriptor | null;
}

function isHighlightRegistry(value: unknown): value is HighlightRegistryLike {
  if (!value || typeof value !== "object") return false;
  return (
    typeof Reflect.get(value, "set") === "function" &&
    typeof Reflect.get(value, "delete") === "function"
  );
}

function isHighlightConstructor(
  value: unknown,
): value is HighlightConstructor {
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

export function useReportHighlights({
  containerRef,
  comments,
  activeThreadId,
  pendingSelection,
}: UseReportHighlightsOptions) {
  const { registry, HighlightCtor } = highlightSupport();
  const supported = Boolean(registry && HighlightCtor);

  useEffect(() => {
    if (!registry || !HighlightCtor) {
      return undefined;
    }
    const container = containerRef.current;
    if (!container) {
      return undefined;
    }
    const commentRanges: Range[] = [];
    const activeRanges: Range[] = [];

    for (const thread of comments) {
      if (thread.status === "archived" || thread.anchor_detached) {
        continue;
      }
      const target =
        thread.thread_id === activeThreadId ? activeRanges : commentRanges;
      const blockIds = commentThreadBlockIds(thread);
      const startBlockId = blockIds[0];
      if (!startBlockId) continue;
      target.push(...rangesForBlock(container, startBlockId, thread.quote));
      const endBlockId = thread.end_block_id?.toString();
      if (endBlockId) {
        target.push(
          ...rangesForBlock(container, endBlockId, thread.end_quote),
        );
      }
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

  return { supported };
}

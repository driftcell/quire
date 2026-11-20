import { useCallback, useEffect, useState } from "react";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import type { SelectionQuoteAction } from "@/components/report/SelectionQuoteToolbar";
import { toViewportRect } from "@/components/report/reportDom";

function findBlockElement(node: Node | null): HTMLElement | null {
  let curr: Node | null = node;
  while (curr) {
    if (curr instanceof HTMLElement && curr.dataset.blockId) {
      return curr;
    }
    curr = curr.parentNode;
  }
  return null;
}

export interface TextSelectionQuoteOptions {
  enabled: boolean;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onAskAgentWithQuote?: (blockId: string, exact: string) => void;
}

export function useTextSelectionQuote({
  enabled,
  onOpenCommentForBlock,
  onAskAgentWithQuote,
}: TextSelectionQuoteOptions) {
  const [quoteAction, setQuoteAction] = useState<SelectionQuoteAction | null>(
    null,
  );

  const handleMouseUp = useCallback(() => {
    if (!enabled || (!onOpenCommentForBlock && !onAskAgentWithQuote)) {
      return;
    }
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
      setQuoteAction(null);
      return;
    }
    const exact = selection.toString().trim();
    if (exact.length === 0) {
      setQuoteAction(null);
      return;
    }
    const range = selection.getRangeAt(0);
    const startBlock = findBlockElement(range.startContainer);
    const endBlock = findBlockElement(range.endContainer);
    if (!startBlock || !endBlock || startBlock !== endBlock) {
      setQuoteAction(null);
      return;
    }
    const blockId = startBlock.dataset.blockId;
    if (!blockId) {
      setQuoteAction(null);
      return;
    }

    const fullText = startBlock.textContent ?? "";
    const matchPos = fullText.indexOf(exact);
    const prefix =
      matchPos > 0
        ? fullText.slice(Math.max(0, matchPos - 24), matchPos)
        : null;
    const suffix =
      matchPos >= 0
        ? fullText.slice(matchPos + exact.length, matchPos + exact.length + 24)
        : null;

    const rect = range.getBoundingClientRect();

    setQuoteAction({
      blockId,
      quote: {
        exact,
        prefix: prefix && prefix.length > 0 ? prefix : undefined,
        suffix: suffix && suffix.length > 0 ? suffix : undefined,
      },
      viewportRect: toViewportRect(rect),
    });
  }, [enabled, onOpenCommentForBlock, onAskAgentWithQuote]);

  useEffect(() => {
    window.document.addEventListener("mouseup", handleMouseUp);
    return () => window.document.removeEventListener("mouseup", handleMouseUp);
  }, [handleMouseUp]);

  return { quoteAction, setQuoteAction };
}

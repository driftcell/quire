import { useCallback, useEffect, useState } from "react";
import type { RefObject } from "react";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import type { SelectionQuoteAction } from "@/components/report/reportSelection";
import {
  buildAnchorSnapshot,
  findBlockElement,
  isTextualBlock,
  normalizeBlockIds,
  placeholderForBlock,
  quoteSelectorFromText,
  textWithinBlock,
  visibleBlockElements,
} from "@/components/report/selectionRange";
import { toViewportRect } from "@/components/report/reportDom";

export interface TextSelectionQuoteOptions {
  enabled: boolean;
  containerRef: RefObject<HTMLElement | null>;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onAskAgentWithQuote?: (selection: SelectionQuoteAction) => void;
}

function intersectsSelection(range: Range, element: HTMLElement): boolean {
  try {
    return range.intersectsNode(element);
  } catch {
    return false;
  }
}

function uniqueBlocksByDocumentOrder(blocks: HTMLElement[]): HTMLElement[] {
  const seen = new Set<string>();
  const result: HTMLElement[] = [];
  for (const block of blocks) {
    const blockId = block.dataset.blockId;
    if (!blockId || seen.has(blockId)) continue;
    seen.add(blockId);
    result.push(block);
  }
  return result;
}

function selectionBlocks(
  container: HTMLElement,
  range: Range,
): HTMLElement[] {
  const candidates = visibleBlockElements(container);
  const startBlock = findBlockElement(range.startContainer);
  const endBlock = findBlockElement(range.endContainer);
  const startIndex = startBlock
    ? candidates.findIndex((candidate) => candidate === startBlock)
    : -1;
  const endIndex = endBlock
    ? candidates.findIndex((candidate) => candidate === endBlock)
    : -1;

  let blocks: HTMLElement[] = [];
  if (startIndex >= 0 && endIndex >= 0 && startIndex <= endIndex) {
    blocks = candidates.slice(startIndex, endIndex + 1);
  } else {
    blocks = candidates.filter((candidate) =>
      intersectsSelection(range, candidate),
    );
  }
  if (startBlock && !blocks.includes(startBlock)) {
    blocks.unshift(startBlock);
  }
  if (endBlock && !blocks.includes(endBlock)) {
    blocks.push(endBlock);
  }
  return uniqueBlocksByDocumentOrder(blocks);
}

function blockSummaryPart(
  block: HTMLElement,
  range: Range,
  startQuote: string | null,
  endQuote: string | null,
  startBlockId: string,
  endBlockId: string,
): string {
  const blockId = block.dataset.blockId;
  if (!blockId) return "";
  if (blockId === startBlockId && startQuote) {
    return startQuote;
  }
  if (blockId === endBlockId && endQuote) {
    return endQuote;
  }
  if (!isTextualBlock(block)) {
    return placeholderForBlock(block);
  }
  const text = textWithinBlock(block, range)?.text.trim();
  return text || placeholderForBlock(block);
}

export function useTextSelectionQuote({
  enabled,
  containerRef,
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
    const range = selection.getRangeAt(0);
    const container = containerRef.current;
    if (!container) {
      setQuoteAction(null);
      return;
    }
    const blocks = selectionBlocks(container, range);
    if (blocks.length === 0) {
      setQuoteAction(null);
      return;
    }

    const startBlock = findBlockElement(range.startContainer) ?? blocks[0];
    const endBlock =
      findBlockElement(range.endContainer) ?? blocks[blocks.length - 1];
    if (!startBlock || !endBlock) {
      setQuoteAction(null);
      return;
    }
    const startBlockId = startBlock.dataset.blockId;
    const endBlockId = endBlock.dataset.blockId;
    if (!startBlockId || !endBlockId) {
      setQuoteAction(null);
      return;
    }

    const startText = isTextualBlock(startBlock)
      ? textWithinBlock(startBlock, range)
      : null;
    const endText =
      isTextualBlock(endBlock) && endBlockId !== startBlockId
        ? textWithinBlock(endBlock, range)
        : null;
    const startSelector =
      startText &&
      quoteSelectorFromText(
        startBlock.textContent ?? "",
        startText.start,
        startText.end,
      );
    const endSelector =
      endText &&
      quoteSelectorFromText(
        endBlock.textContent ?? "",
        endText.start,
        endText.end,
      );

    const quote =
      startSelector ??
      ({
        exact: placeholderForBlock(startBlock),
      } satisfies SelectionQuoteAction["quote"]);
    const endQuote =
      endSelector ??
      (endBlockId !== startBlockId && !isTextualBlock(endBlock)
        ? {
            exact: placeholderForBlock(endBlock),
          }
        : undefined);

    const anchorSnapshot = buildAnchorSnapshot(
      blocks.map((block) =>
        blockSummaryPart(
          block,
          range,
          startSelector?.exact ?? (isTextualBlock(startBlock) ? null : quote.exact),
          endQuote?.exact ?? null,
          startBlockId,
          endBlockId,
        ),
      ),
    );
    const blockIds = normalizeBlockIds(
      blocks.map((block) => block.dataset.blockId ?? ""),
    );
    const rect = range.getBoundingClientRect();
    const fallbackRect = startBlock.getBoundingClientRect();
    const usableRect =
      rect.width > 0 || rect.height > 0 ? rect : fallbackRect;

    setQuoteAction({
      blockId: startBlockId,
      ...(endBlockId !== startBlockId ? { endBlockId } : {}),
      blockIds,
      quote,
      ...(endQuote ? { endQuote } : {}),
      anchorSnapshot,
      viewportRect: toViewportRect(usableRect),
    });
  }, [
    containerRef,
    enabled,
    onOpenCommentForBlock,
    onAskAgentWithQuote,
  ]);

  useEffect(() => {
    window.document.addEventListener("mouseup", handleMouseUp);
    return () => window.document.removeEventListener("mouseup", handleMouseUp);
  }, [handleMouseUp]);

  return { quoteAction, setQuoteAction };
}

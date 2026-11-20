import type { TextQuoteSelector } from "@/contracts/report-protocol";

export interface ReportSelectionDescriptor {
  /** Starting block. */
  blockId: string;
  /** Ending block when the selection crosses block boundaries. */
  endBlockId?: string;
  /** Every selected block in document order, including media blocks. */
  blockIds: string[];
  /** Start-block quote with prefix/suffix context. */
  quote: TextQuoteSelector;
  /** End-block quote for cross-block selections. */
  endQuote?: TextQuoteSelector;
  /** Display/agent summary, including `[表格: …]` / `[图表: …]` placeholders. */
  anchorSnapshot: string;
}

export interface CommentThreadBlocks {
  block_id: string;
  end_block_id?: string | null;
  anchor_blocks?: string[];
}

export function commentThreadBlockIds(thread: CommentThreadBlocks): string[] {
  if ((thread.anchor_blocks?.length ?? 0) > 0) {
    return thread.anchor_blocks ?? [];
  }
  return [
    thread.block_id,
    ...(thread.end_block_id ? [thread.end_block_id] : []),
  ];
}

export interface SelectionQuoteAction extends ReportSelectionDescriptor {
  viewportRect: {
    top: number;
    left: number;
    bottom: number;
    right: number;
    width: number;
    height: number;
  };
}

export function selectionRangeLabel(
  selection: Pick<ReportSelectionDescriptor, "blockId" | "endBlockId">,
): string {
  return selection.endBlockId
    ? `#${selection.blockId}…#${selection.endBlockId}`
    : `#${selection.blockId}`;
}

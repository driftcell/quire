import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";

export function toViewportRect(
  domRect: DOMRect,
): FloatingCommentAnchor["rect"] {
  return {
    top: domRect.top,
    left: domRect.left,
    bottom: domRect.bottom,
    right: domRect.right,
    width: domRect.width,
    height: domRect.height,
  };
}

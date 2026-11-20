import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";

const flashTimers = new WeakMap<HTMLElement, number>();

export function flashHighlightElement(
  el: HTMLElement,
  durationMs = 3000,
): void {
  const existing = flashTimers.get(el);
  if (existing !== undefined) {
    window.clearTimeout(existing);
  }
  el.classList.remove("quire-flash-highlight");
  void el.offsetWidth;
  el.classList.add("quire-flash-highlight");
  const timer = window.setTimeout(() => {
    el.classList.remove("quire-flash-highlight");
    flashTimers.delete(el);
  }, durationMs);
  flashTimers.set(el, timer);
}

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

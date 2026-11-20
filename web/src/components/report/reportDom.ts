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

/**
 * Base UI positions a popover against any Floating UI virtual element. Our
 * selection and comment anchors already store viewport coordinates, so expose
 * them through the minimal virtual-element contract instead of re-importing
 * Floating UI middleware in each caller.
 */
export function toPopoverAnchor(rect: FloatingCommentAnchor["rect"]) {
  return {
    getBoundingClientRect: () => ({
      x: rect.left,
      y: rect.top,
      width: rect.width,
      height: rect.height,
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      left: rect.left,
    }),
  };
}

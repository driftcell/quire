import { useEffect, useState, type RefObject } from "react";
import { A4_PAGE_WIDTH_PX } from "@/components/report/pagination";

export function usePageScale(
  rootRef: RefObject<HTMLDivElement | null>,
  isPaged: boolean,
  suppressChrome: boolean,
): number {
  const [pageScale, setPageScale] = useState(1);

  useEffect(() => {
    if (!isPaged || suppressChrome) {
      const resetFrame = window.requestAnimationFrame(() => setPageScale(1));
      return () => window.cancelAnimationFrame(resetFrame);
    }
    const el = rootRef.current;
    if (!el) return undefined;

    const updateScale = (width: number) => {
      if (width <= 0) return;
      const next = Math.min(1, Math.max(0.45, (width - 32) / A4_PAGE_WIDTH_PX));
      setPageScale(next);
    };

    const frame = window.requestAnimationFrame(() => {
      updateScale(el.getBoundingClientRect().width);
    });
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        updateScale(entry.contentRect.width);
      }
    });
    observer.observe(el);
    return () => {
      window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [isPaged, rootRef, suppressChrome]);

  return pageScale;
}

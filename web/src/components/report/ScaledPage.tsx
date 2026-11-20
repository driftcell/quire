import { useLayoutEffect, useRef, type ReactNode } from "react";
import { A4_PAGE_WIDTH_PX } from "@/components/report/pagination";

// Scale the fixed-width A4 page with a compositor transform. CSS `zoom` would
// reflow text at fractional scales, making the page height and line wrapping
// jitter while the panel is resized. The outer box reserves the scaled size.
export function ScaledPage({
  scale,
  plain = false,
  children,
}: {
  scale: number;
  plain?: boolean;
  children: ReactNode;
}) {
  const outerRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (plain) {
      return undefined;
    }
    const outer = outerRef.current;
    const inner = innerRef.current;
    if (!outer || !inner) {
      return undefined;
    }
    const apply = () => {
      outer.style.width = `${A4_PAGE_WIDTH_PX * scale}px`;
      outer.style.height = `${inner.offsetHeight * scale}px`;
    };
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(inner);
    return () => observer.disconnect();
  }, [scale, plain]);

  if (plain) {
    return <>{children}</>;
  }

  return (
    <div ref={outerRef} className="relative shrink-0">
      <div
        ref={innerRef}
        style={{
          width: `${A4_PAGE_WIDTH_PX}px`,
          transform: `translateZ(0) scale(${scale})`,
          transformOrigin: "top left",
          willChange: "transform",
          backfaceVisibility: "hidden",
        }}
      >
        {children}
      </div>
    </div>
  );
}

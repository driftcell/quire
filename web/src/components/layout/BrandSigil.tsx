import { BrandMark } from "@/components/layout/BrandMark";

export interface BrandSigilProps {
  className?: string;
}

/**
 * Editorial Masthead Sigil for Quire.
 * Replaces the generic neon glowing hexagon with a crisp architectural folio
 * plate and ruled editorial register marks.
 */
export function BrandSigil({ className }: BrandSigilProps) {
  return (
    <div
      className={`brand-sigil${className ? ` ${className}` : ""}`}
      aria-hidden="true"
    >
      <div className="relative flex size-14 items-center justify-center rounded-2xl border border-border/80 bg-card shadow-xs">
        <BrandMark className="size-9" />
      </div>
    </div>
  );
}

import { BrandMark } from "@/components/layout/BrandMark";

export interface BrandSigilProps {
  className?: string;
}

export function BrandSigil({ className }: BrandSigilProps) {
  return (
    <div
      className={`brand-sigil${className ? ` ${className}` : ""}`}
      aria-hidden="true"
    >
      <span className="brand-sigil-glow" />
      <BrandMark className="brand-sigil-mark" />
    </div>
  );
}

import { BrandMark } from "@/components/layout/BrandMark";

export interface ProductBrandProps {
  description?: string;
  compact?: boolean;
}

export function ProductBrand({
  description,
  compact = false,
}: ProductBrandProps) {
  return (
    <div className="flex min-w-0 items-center gap-2.5">
      <BrandMark className="size-6 shrink-0" />
      <div className="min-w-0">
        <div className="flex items-baseline gap-1.5">
          <span className="font-serif text-[16px] leading-none font-bold tracking-tight text-foreground">
            Quire
          </span>
        </div>
        {description && !compact && (
          <p className="mt-0.5 truncate text-[10.5px] text-muted-foreground">
            {description}
          </p>
        )}
      </div>
    </div>
  );
}

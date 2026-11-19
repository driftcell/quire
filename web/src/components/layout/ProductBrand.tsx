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
      <BrandMark className="size-7 shrink-0" />
      <div className="min-w-0">
        <p className="truncate text-sm font-semibold tracking-tight">
          青塔 · 智能报告助手
        </p>
        {description && !compact && (
          <p className="mt-0.5 truncate text-[10px] text-muted-foreground">
            {description}
          </p>
        )}
      </div>
    </div>
  );
}

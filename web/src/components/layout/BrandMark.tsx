export interface BrandMarkProps {
  className?: string;
}

export function BrandMark({ className }: BrandMarkProps) {
  return (
    <svg
      viewBox="0 0 48 48"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <path
        className="brand-mark-face"
        d="M24 3 42.19 13.5v21L24 45 5.81 34.5v-21Z"
      />
      <g className="brand-mark-bars">
        <path d="M15.5 19v10" />
        <path d="M20 14v20" />
        <path d="M24.5 10v28" />
        <path d="M29 14v20" />
        <path d="M33.5 19v10" />
      </g>
    </svg>
  );
}

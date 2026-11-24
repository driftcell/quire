export interface BrandMarkProps {
  className?: string;
}

/**
 * Quire Brand Mark — "The Editorial Quire"
 * A folded manuscript quire (layered folio sheets) with precision editorial
 * ruled lines and an Oxford-blue Q tail / colophon seal.
 */
export function BrandMark({ className }: BrandMarkProps) {
  return (
    <svg
      viewBox="0 0 48 48"
      fill="none"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <rect
        x="3"
        y="3"
        width="42"
        height="42"
        rx="10"
        className="fill-[#141413] dark:fill-[#FAF9F6]"
      />
      <path
        d="M12.5 14C12.5 13.1716 13.1716 12.5 14 12.5H25C27.4853 12.5 29.5 14.5147 29.5 17V33.5C29.5 32.1193 28.3807 31 27 31H14C13.1716 31 12.5 30.3284 12.5 29.5V14Z"
        className="fill-[#FAF9F6] dark:fill-[#141413]"
      />
      <path
        d="M29.5 17C29.5 15.3431 30.8431 14 32.5 14H34.5C35.3284 14 36 14.6716 36 15.5V30C36 30.8284 35.3284 31.5 34.5 31.5H31.2C30.2611 31.5 29.5 32.2611 29.5 33.2V17Z"
        className="fill-[#D6D3CD] dark:fill-[#3F3E3A]"
      />
      <path
        d="M16.5 18H25"
        className="stroke-[#141413] dark:stroke-[#FAF9F6]"
        strokeWidth="1.85"
        strokeLinecap="round"
      />
      <path
        d="M16.5 22H25"
        className="stroke-[#141413] dark:stroke-[#FAF9F6]"
        strokeWidth="1.85"
        strokeLinecap="round"
      />
      <path
        d="M16.5 26H22"
        className="stroke-[#141413] dark:stroke-[#FAF9F6]"
        strokeWidth="1.85"
        strokeLinecap="round"
      />
      <path
        d="M27 28L34.5 35.5"
        className="stroke-[#1D4ED8]"
        strokeWidth="2.8"
        strokeLinecap="round"
      />
      <circle cx="34.5" cy="35.5" r="2" className="fill-[#1D4ED8]" />
    </svg>
  );
}

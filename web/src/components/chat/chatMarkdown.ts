import type { AnimateOptions, ControlsConfig } from "streamdown";

export const CHAT_PROSE =
  "prose prose-sm max-w-none prose-headings:font-semibold prose-headings:mt-3 prose-headings:mb-1.5 prose-h1:text-base prose-h2:text-[15px] prose-h3:text-sm prose-p:my-1.5 prose-p:leading-6 prose-ul:my-1.5 prose-ol:my-1.5 prose-pre:my-2 prose-pre:text-xs prose-code:text-[12.5px]";

export const STREAMDOWN_CONTROLS: ControlsConfig = {
  table: false,
  code: true,
};

export const STREAMDOWN_ANIMATION: AnimateOptions = {
  animation: "fadeIn",
  duration: 120,
  easing: "ease-out",
  sep: "char",
};

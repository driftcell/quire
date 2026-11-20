import { useEffect, useState } from "react";
import type { PageChromeConfig } from "@/contracts/report-protocol";
import { isRecord } from "@/lib/guards";

export type HeadingPageBreak = NonNullable<
  PageChromeConfig["heading_page_break"]
>;

export interface PageChromeSlotsValue {
  [key: string]: unknown;
  left: string;
  center: string;
  right: string;
}

export interface ResolvedPageChrome {
  header: PageChromeSlotsValue;
  footer: PageChromeSlotsValue;
  hide_header_on_first_page: boolean;
  hide_footer_on_first_page: boolean;
  heading_page_break: HeadingPageBreak;
}

export interface PageChromePreset {
  id: "classic" | "centered" | "official";
  label: string;
  config: ResolvedPageChrome;
}

export interface PageChromeTemplateContext {
  page: number;
  total: number;
  title: string;
  subtitle: string;
  date: string;
  kicker: string;
  authors: string;
}

export const PAGE_CHROME_CHANGED_EVENT = "quire:page-chrome-changed";

const STORAGE_PREFIX = "quire.pageChrome.v1.";

function emptySlots(): PageChromeSlotsValue {
  return { left: "", center: "", right: "" };
}

function cloneChrome(config: ResolvedPageChrome): ResolvedPageChrome {
  return {
    header: { ...config.header },
    footer: { ...config.footer },
    hide_header_on_first_page: config.hide_header_on_first_page,
    hide_footer_on_first_page: config.hide_footer_on_first_page,
    heading_page_break: config.heading_page_break,
  };
}

function readHeadingPageBreak(value: unknown): HeadingPageBreak {
  return value === "h1" || value === "h1_and_h2" ? value : "none";
}

const CLASSIC: ResolvedPageChrome = {
  header: emptySlots(),
  footer: {
    left: "{title}",
    center: "",
    right: "第 {page} 页 / 共 {total} 页",
  },
  hide_header_on_first_page: true,
  hide_footer_on_first_page: false,
  heading_page_break: "none",
};

const CENTERED: ResolvedPageChrome = {
  header: emptySlots(),
  footer: {
    left: "",
    center: "— {page} / {total} —",
    right: "",
  },
  hide_header_on_first_page: true,
  hide_footer_on_first_page: false,
  heading_page_break: "none",
};

const OFFICIAL: ResolvedPageChrome = {
  header: {
    left: "{kicker}",
    center: "",
    right: "{title}",
  },
  footer: {
    left: "{authors}",
    center: "第 {page} 页 / 共 {total} 页",
    right: "{date}",
  },
  hide_header_on_first_page: true,
  hide_footer_on_first_page: false,
  heading_page_break: "none",
};

export const PAGE_CHROME_PRESETS: PageChromePreset[] = [
  { id: "classic", label: "左题右码", config: cloneChrome(CLASSIC) },
  { id: "centered", label: "居中页码", config: cloneChrome(CENTERED) },
  { id: "official", label: "学术公文全配", config: cloneChrome(OFFICIAL) },
];

function readSlots(value: unknown): PageChromeSlotsValue {
  if (!isRecord(value)) {
    return emptySlots();
  }
  return {
    left: typeof value.left === "string" ? value.left : "",
    center: typeof value.center === "string" ? value.center : "",
    right: typeof value.right === "string" ? value.right : "",
  };
}

export function normalizePageChromeConfig(
  config?: PageChromeConfig | ResolvedPageChrome | null,
): ResolvedPageChrome {
  if (!config) {
    return cloneChrome(CLASSIC);
  }
  return {
    header: readSlots(config.header),
    footer: readSlots(config.footer),
    hide_header_on_first_page: config.hide_header_on_first_page ?? true,
    hide_footer_on_first_page: config.hide_footer_on_first_page ?? false,
    heading_page_break: readHeadingPageBreak(config.heading_page_break),
  };
}

function storageKey(docId: string): string {
  return `${STORAGE_PREFIX}${docId}`;
}

function loadLocalPageChrome(docId: string): ResolvedPageChrome | null {
  try {
    const raw = window.localStorage.getItem(storageKey(docId));
    if (!raw) {
      return null;
    }
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) {
      return null;
    }
    return normalizePageChromeConfig({
      header: readSlots(parsed.header),
      footer: readSlots(parsed.footer),
      hide_header_on_first_page:
        typeof parsed.hide_header_on_first_page === "boolean"
          ? parsed.hide_header_on_first_page
          : true,
      hide_footer_on_first_page:
        typeof parsed.hide_footer_on_first_page === "boolean"
          ? parsed.hide_footer_on_first_page
          : false,
      heading_page_break: readHeadingPageBreak(parsed.heading_page_break),
    });
  } catch {
    return null;
  }
}

export function resolvePageChromeConfig(
  docId: string,
  themePageChrome?: PageChromeConfig | null,
): ResolvedPageChrome {
  const local = loadLocalPageChrome(docId);
  if (local) {
    return local;
  }
  if (themePageChrome) {
    return normalizePageChromeConfig(themePageChrome);
  }
  return cloneChrome(CLASSIC);
}

function notifyPageChromeChanged(): void {
  window.dispatchEvent(new Event(PAGE_CHROME_CHANGED_EVENT));
}

export function savePageChromeConfig(
  docId: string,
  config: PageChromeConfig | ResolvedPageChrome,
): void {
  try {
    window.localStorage.setItem(
      storageKey(docId),
      JSON.stringify(normalizePageChromeConfig(config)),
    );
  } catch {
    // Ignore storage quota / privacy mode errors.
  }
  notifyPageChromeChanged();
}

export function clearPageChromeConfig(docId: string): void {
  try {
    window.localStorage.removeItem(storageKey(docId));
  } catch {
    // Ignore storage quota / privacy mode errors.
  }
  notifyPageChromeChanged();
}

export function formatPageTemplate(
  template: string,
  context: PageChromeTemplateContext,
): string {
  return template
    .replaceAll("{page}", String(context.page))
    .replaceAll("{total}", String(context.total))
    .replaceAll("{title}", context.title)
    .replaceAll("{subtitle}", context.subtitle)
    .replaceAll("{date}", context.date)
    .replaceAll("{kicker}", context.kicker)
    .replaceAll("{authors}", context.authors);
}

export function hasPageHeader(config: ResolvedPageChrome): boolean {
  return Boolean(
    config.header.left.trim() ||
    config.header.center.trim() ||
    config.header.right.trim(),
  );
}

export function hasPageFooter(config: ResolvedPageChrome): boolean {
  return Boolean(
    config.footer.left.trim() ||
    config.footer.center.trim() ||
    config.footer.right.trim(),
  );
}

/**
 * Resolve the active page chrome and re-render when the local override changes.
 */
export function usePageChromeConfig(
  docId: string,
  themePageChrome?: PageChromeConfig | null,
): ResolvedPageChrome {
  const [, setVersion] = useState(0);

  useEffect(() => {
    const handleChange = () => {
      setVersion((version) => version + 1);
    };
    window.addEventListener(PAGE_CHROME_CHANGED_EVENT, handleChange);
    return () => {
      window.removeEventListener(PAGE_CHROME_CHANGED_EVENT, handleChange);
    };
  }, []);

  return resolvePageChromeConfig(docId, themePageChrome);
}

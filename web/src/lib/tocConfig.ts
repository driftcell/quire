import { useEffect, useState } from "react";
import type {
  ReferencesConfig,
  TocConfig,
  TocNode,
} from "@/contracts/report-protocol";
import { isRecord } from "@/lib/guards";

export type TocVariant = "standard" | "numbered" | "dotted";
export type TocMaxDepth = 1 | 2 | 3;

export interface ResolvedTocConfig {
  enabled: boolean;
  title: string;
  max_depth: TocMaxDepth;
  variant: TocVariant;
}

export const TOC_CONFIG_CHANGED_EVENT = "quire:toc-config-changed";

const STORAGE_PREFIX = "quire.tocConfig.v1.";
const DEFAULT_TOC_TITLE = "目录";
const DEFAULT_TOC_MAX_DEPTH: TocMaxDepth = 3;

const DEFAULT_TITLES = new Set([
  "contents",
  "table of contents",
  "index",
  "目录",
]);

export const TOC_MAX_DEPTH_OPTIONS: TocMaxDepth[] = [1, 2, 3];

export const TOC_VARIANT_OPTIONS: Array<{
  value: TocVariant;
  label: string;
  description: string;
}> = [
  {
    value: "dotted",
    label: "点线页码",
    description: "标题与右侧页码之间使用点状引导线",
  },
  {
    value: "numbered",
    label: "章节编号",
    description: "在标题前显示协议生成的章节号",
  },
  {
    value: "standard",
    label: "简洁",
    description: "仅显示标题文本，不显示章节号",
  },
];

function storageKey(docId: string): string {
  return `${STORAGE_PREFIX}${docId}`;
}

function normalizeTocTitle(raw: unknown): string {
  const title = typeof raw === "string" ? raw.trim() : "";
  if (!title || DEFAULT_TITLES.has(title.toLowerCase())) {
    return DEFAULT_TOC_TITLE;
  }
  return title;
}

export function clampTocMaxDepth(value: unknown): TocMaxDepth {
  const numeric =
    typeof value === "number" && Number.isFinite(value)
      ? Math.round(value)
      : DEFAULT_TOC_MAX_DEPTH;
  if (numeric <= 1) return 1;
  if (numeric >= 3) return 3;
  return 2;
}

function isTocVariant(value: unknown): value is TocVariant {
  return value === "standard" || value === "numbered" || value === "dotted";
}

function normalizeTocVariant(value: unknown, paged: boolean): TocVariant {
  if (isTocVariant(value)) {
    return value;
  }
  // Protocol default is `standard`. In paged/print output the extra page
  // numbers make the dotted-leader layout the most useful default; an explicit
  // `standard`/`numbered` from the document or local override always wins.
  return paged ? "dotted" : "standard";
}

export interface NormalizeTocOptions {
  paged?: boolean;
}

export function normalizeTocConfig(
  config?: TocConfig | ResolvedTocConfig | null,
  options: NormalizeTocOptions = {},
): ResolvedTocConfig {
  return {
    enabled: config?.enabled ?? true,
    title: normalizeTocTitle(config?.title),
    max_depth: clampTocMaxDepth(config?.max_depth),
    variant: normalizeTocVariant(config?.variant, options.paged ?? false),
  };
}

interface TocLocalOverride {
  enabled?: boolean;
  max_depth?: TocMaxDepth;
  variant?: TocVariant;
}

function readLocalTocOverride(docId: string): TocLocalOverride | null {
  try {
    const raw = window.localStorage.getItem(storageKey(docId));
    if (!raw) {
      return null;
    }
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) {
      return null;
    }
    const override: TocLocalOverride = {};
    if (typeof parsed.enabled === "boolean") {
      override.enabled = parsed.enabled;
    }
    if (parsed.max_depth !== undefined) {
      override.max_depth = clampTocMaxDepth(parsed.max_depth);
    }
    if (isTocVariant(parsed.variant)) {
      override.variant = parsed.variant;
    }
    return Object.keys(override).length > 0 ? override : null;
  } catch {
    return null;
  }
}

export function resolveTocConfig(
  docId: string,
  documentToc?: TocConfig | ResolvedTocConfig | null,
  paged = false,
): ResolvedTocConfig {
  const base = normalizeTocConfig(documentToc, { paged });
  const local = readLocalTocOverride(docId);
  if (!local) {
    return base;
  }
  return {
    ...base,
    ...local,
  };
}

function notifyTocConfigChanged(): void {
  window.dispatchEvent(new Event(TOC_CONFIG_CHANGED_EVENT));
}

export function saveTocConfig(
  docId: string,
  config: TocConfig | ResolvedTocConfig,
): void {
  const normalized = normalizeTocConfig(config);
  saveTocConfigPatch(docId, {
    enabled: normalized.enabled,
    max_depth: normalized.max_depth,
    variant: normalized.variant,
  });
}

export type TocConfigPatch = Partial<
  Pick<ResolvedTocConfig, "enabled" | "max_depth" | "variant">
>;

/**
 * Persist only the fields the user explicitly changed. Keeping the override
 * partial lets the document variant / paged-mode default keep applying to the
 * fields the user has not touched.
 */
export function saveTocConfigPatch(docId: string, patch: TocConfigPatch): void {
  const next: TocLocalOverride = readLocalTocOverride(docId) ?? {};
  if (patch.enabled !== undefined) {
    next.enabled = patch.enabled;
  }
  if (patch.max_depth !== undefined) {
    next.max_depth = clampTocMaxDepth(patch.max_depth);
  }
  if (patch.variant !== undefined) {
    next.variant = patch.variant;
  }
  try {
    window.localStorage.setItem(storageKey(docId), JSON.stringify(next));
  } catch {
    // Ignore storage quota / privacy mode errors.
  }
  notifyTocConfigChanged();
}

export function clearTocConfig(docId: string): void {
  try {
    window.localStorage.removeItem(storageKey(docId));
  } catch {
    // Ignore storage quota / privacy mode errors.
  }
  notifyTocConfigChanged();
}

/**
 * Resolve the active inline-TOC config and re-render when the local override
 * changes (for example from the outline popover).
 */
export function useTocConfig(
  docId: string,
  documentToc?: TocConfig | ResolvedTocConfig | null,
  paged = false,
): ResolvedTocConfig {
  const [, setVersion] = useState(0);

  useEffect(() => {
    const handleChange = () => {
      setVersion((version) => version + 1);
    };
    window.addEventListener(TOC_CONFIG_CHANGED_EVENT, handleChange);
    return () => {
      window.removeEventListener(TOC_CONFIG_CHANGED_EVENT, handleChange);
    };
  }, []);

  return resolveTocConfig(docId, documentToc, paged);
}

/**
 * Recursively keep only heading levels within `maxDepth`. Filtering is done in
 * the browser so the 1/2/3 depth controls take effect without a backend
 * recomputation of `computed.toc_tree`.
 */
export function filterTocNodes(nodes: TocNode[], maxDepth: number): TocNode[] {
  const capped = clampTocMaxDepth(maxDepth);
  const out: TocNode[] = [];
  for (const node of nodes) {
    if (node.level <= capped) {
      const children = node.children ?? [];
      out.push({
        ...node,
        children:
          children.length > 0 ? filterTocNodes(children, capped) : children,
      });
    }
  }
  return out;
}

const LEADING_CHAPTER_NUMBER_RE =
  /^(?:第[一二三四五六七八九十百零〇两0-9]+[章节篇部]|[一二三四五六七八九十百零〇]+[、.．]|[(（][一二三四五六七八九十0-9]+[)）]|[0-9]+(?:\.[0-9]+)*[、.．)）])/;

/**
 * Whether a heading already carries a chapter number in its text. Used to
 * avoid duplicate prefixes such as `1 1. 总体概况` in numbered/dotted TOCs.
 */
export function tocHeadingHasNumber(node: TocNode): boolean {
  const text = node.text.trim();
  const number = node.number.trim();
  if (number && text.startsWith(number)) {
    const rest = text.slice(number.length);
    if (rest.length === 0 || /^[\s、.．)）:：\-—]/.test(rest)) {
      return true;
    }
  }
  return LEADING_CHAPTER_NUMBER_RE.test(text);
}

const DEFAULT_REFERENCES_TITLE = "参考资料";

export function normalizeReferencesTitle(
  config?: ReferencesConfig | null,
): string {
  const title = config?.title?.trim() ?? "";
  if (!title) {
    return DEFAULT_REFERENCES_TITLE;
  }
  const lowered = title.toLowerCase();
  if (lowered === "references" || lowered === "bibliography") {
    return DEFAULT_REFERENCES_TITLE;
  }
  return title;
}

export function referencesPageBreakBefore(
  config?: ReferencesConfig | null,
): boolean {
  return config?.page_break_before ?? true;
}

export function referencesEnabled(config?: ReferencesConfig | null): boolean {
  return config?.enabled ?? true;
}

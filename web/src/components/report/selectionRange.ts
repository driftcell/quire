import type { TextQuoteSelector } from "@/contracts/report-protocol";

const CONTEXT_CHARS = 24;
const MAX_EXACT_CHARS = 2_000;
const MAX_SNAPSHOT_CHARS = 2_000;
const MAX_PLACEHOLDER_TITLE = 80;

interface TextNodeEntry {
  node: Node;
  start: number;
  end: number;
}

interface BlockText {
  text: string;
  start: number;
  end: number;
}

function isBlockElement(node: Node | null): node is HTMLElement {
  return node instanceof HTMLElement && Boolean(node.dataset.blockId);
}

/** Find the nearest report block, crossing a shadow-root host boundary when needed. */
export function findBlockElement(node: Node | null): HTMLElement | null {
  let current: Node | null = node;
  while (current) {
    if (isBlockElement(current)) {
      return current;
    }
    const host = (current as Node & { host?: Node }).host;
    current = host ?? current.parentNode;
  }
  return null;
}

export function visibleBlockElements(container: HTMLElement): HTMLElement[] {
  const elements = Array.from(
    container.querySelectorAll<HTMLElement>("[data-block-id]"),
  );
  return elements.filter(
    (element) => element.dataset.blockId && !element.closest("[data-measure-root]"),
  );
}

export function normalizeBlockIds(blockIds: string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const blockId of blockIds) {
    const trimmed = blockId.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    normalized.push(trimmed);
  }
  return normalized;
}

export function buildAnchorSnapshot(parts: string[]): string {
  const text = parts
    .map((part) => part.replace(/\s+/g, " ").trim())
    .filter(Boolean)
    .join(" … ");
  if (text.length <= MAX_SNAPSHOT_CHARS) {
    return text;
  }
  return `${text.slice(0, MAX_SNAPSHOT_CHARS - 1)}…`;
}

export function quoteSelectorFromText(
  fullText: string,
  start: number,
  end: number,
): TextQuoteSelector | null {
  const safeStart = Math.max(0, Math.min(start, fullText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, fullText.length));
  let exactStart = safeStart;
  let exactEnd = safeEnd;
  while (exactStart < exactEnd && /\s/.test(fullText[exactStart] ?? "")) {
    exactStart += 1;
  }
  while (exactEnd > exactStart && /\s/.test(fullText[exactEnd - 1] ?? "")) {
    exactEnd -= 1;
  }
  const exact = fullText.slice(exactStart, exactEnd);
  if (!exact.trim()) {
    return null;
  }
  const prefix = fullText.slice(
    Math.max(0, exactStart - CONTEXT_CHARS),
    exactStart,
  );
  const suffix = fullText.slice(
    exactEnd,
    Math.min(fullText.length, exactEnd + CONTEXT_CHARS),
  );
  return {
    exact: exact.slice(0, MAX_EXACT_CHARS),
    ...(prefix ? { prefix } : {}),
    ...(suffix ? { suffix } : {}),
  };
}

function textNodes(root: Node): TextNodeEntry[] {
  const document = root.ownerDocument;
  if (!document) return [];
  const walker = document.createTreeWalker(root, 4);
  const entries: TextNodeEntry[] = [];
  let offset = 0;
  let current = walker.nextNode();
  while (current) {
    if (current.nodeType === 3) {
      const value = current.textContent ?? "";
      if (value.length > 0) {
        entries.push({ node: current, start: offset, end: offset + value.length });
        offset += value.length;
      }
    }
    current = walker.nextNode();
  }
  return entries;
}

function boundaryAt(
  entries: TextNodeEntry[],
  offset: number,
): { node: Node; offset: number } | null {
  for (const entry of entries) {
    if (offset <= entry.end) {
      return {
        node: entry.node,
        offset: Math.max(0, Math.min(offset - entry.start, entry.end - entry.start)),
      };
    }
  }
  const last = entries[entries.length - 1];
  return last ? { node: last.node, offset: last.end - last.start } : null;
}

/** Locate a text quote inside one block using prefix/suffix disambiguation. */
export function locateQuoteRange(
  root: HTMLElement,
  selector: TextQuoteSelector,
): Range | null {
  const entries = textNodes(root);
  if (entries.length === 0) {
    return null;
  }
  const combined = entries
    .map((entry) => entry.node.textContent ?? "")
    .join("");
  const exact = selector.exact;
  if (!exact) {
    return null;
  }
  const prefix = selector.prefix ?? "";
  const suffix = selector.suffix ?? "";
  let bestIndex = -1;
  let bestScore = -1;
  let candidate = combined.indexOf(exact);
  while (candidate !== -1) {
    let score = 0;
    if (
      prefix &&
      combined.slice(Math.max(0, candidate - prefix.length), candidate) === prefix
    ) {
      score += 2;
    }
    if (
      suffix &&
      combined.slice(candidate + exact.length, candidate + exact.length + suffix.length) ===
        suffix
    ) {
      score += 1;
    }
    if (score > bestScore) {
      bestIndex = candidate;
      bestScore = score;
    }
    if (score === 3) {
      break;
    }
    candidate = combined.indexOf(exact, candidate + 1);
  }
  if (bestIndex < 0) {
    return null;
  }
  const start = boundaryAt(entries, bestIndex);
  const end = boundaryAt(entries, bestIndex + exact.length);
  if (!start || !end) {
    return null;
  }
  const range = root.ownerDocument.createRange();
  range.setStart(start.node, start.offset);
  range.setEnd(end.node, end.offset);
  return range;
}

function boundaryInsideBlock(block: HTMLElement, node: Node): boolean {
  return findBlockElement(node) === block;
}

function offsetWithinBlock(
  block: HTMLElement,
  node: Node,
  offset: number,
): number {
  const range = block.ownerDocument.createRange();
  range.selectNodeContents(block);
  try {
    range.setEnd(node, offset);
  } catch {
    return 0;
  }
  return range.toString().length;
}

/** Selected text within one block, with offsets into the block's light-DOM text. */
export function textWithinBlock(
  block: HTMLElement,
  selection: Range,
): BlockText | null {
  const fullText = block.textContent ?? "";
  const startsInside = boundaryInsideBlock(block, selection.startContainer);
  const endsInside = boundaryInsideBlock(block, selection.endContainer);
  let intersects = startsInside || endsInside;
  if (!intersects) {
    try {
      intersects = selection.intersectsNode(block);
    } catch {
      intersects = false;
    }
  }
  if (!intersects) {
    return null;
  }
  const start = startsInside
    ? offsetWithinBlock(block, selection.startContainer, selection.startOffset)
    : 0;
  const end = endsInside
    ? offsetWithinBlock(block, selection.endContainer, selection.endOffset)
    : fullText.length;
  const safeStart = Math.max(0, Math.min(start, fullText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, fullText.length));
  return {
    text: fullText.slice(safeStart, safeEnd),
    start: safeStart,
    end: safeEnd,
  };
}

export function isTextualBlock(block: HTMLElement): boolean {
  return (
    block.dataset.blockKind === "heading" ||
    block.dataset.blockKind === "paragraph" ||
    block.dataset.blockKind === "quote"
  );
}

function blockTitle(block: HTMLElement): string {
  const raw = block.querySelector("figcaption")?.textContent?.trim() ?? "";
  const title = raw.replace(/^(?:表|图)\s*\d+\s*(?:[:：]\s*)?/, "").trim();
  return title.length > MAX_PLACEHOLDER_TITLE
    ? `${title.slice(0, MAX_PLACEHOLDER_TITLE - 1)}…`
    : title;
}

/** Render a non-text block as a stable selection placeholder. */
export function placeholderForBlock(block: HTMLElement): string {
  const title = blockTitle(block);
  const suffix = title ? `: ${title}` : "";
  switch (block.dataset.blockKind) {
    case "table":
      return `[表格${suffix}]`;
    case "echarts":
      return `[图表${suffix}]`;
    case "custom_html":
      return `[图片${suffix}]`;
    default: {
      const text = (block.textContent ?? "").replace(/\s+/g, " ").trim();
      return text.length > 160 ? `${text.slice(0, 159)}…` : text;
    }
  }
}

import type {
  Block,
  ComputedDocumentMeta,
  CoverConfig,
  ReferencesConfig,
  ReportDocument,
  DocumentSyncDelta,
  SourceEntry,
  ThemeConfig,
  TocConfig,
} from "@/contracts/report-protocol";
import { isRecord, isStringArray } from "@/lib/guards";

/**
 * A block whose `id` is guaranteed to be present after normalization.
 *
 * The wire schema marks `id` optional because agents may omit it on insert and
 * the server assigns one, but every document returned by `GET /api/docs/{id}`
 * already has stable IDs.
 */
export type ReportBlock = Block & { id: string };

/**
 * The ECharts branch of the generated block union. Used by the standalone chart
 * preview route, which receives a chart payload without a full report wrapper.
 */
export type EchartsBlock = Omit<Extract<Block, { type: "echarts" }>, "type"> & {
  type?: "echarts";
};

/**
 * Fully populated document view. The protocol schema marks most top-level
 * fields optional because agents may submit partial documents; the server
 * always serializes them, so the frontend normalizes them to required fields.
 */
export type ReportDocumentView = Omit<
  ReportDocument,
  "blocks" | "cover" | "toc" | "theme" | "references" | "revision"
> & {
  doc_id: string;
  session_id: string | null;
  blocks: ReportBlock[];
  cover: CoverConfig;
  toc: TocConfig;
  theme: ThemeConfig;
  references: ReferencesConfig;
  revision: number;
};

export interface DocumentResponse {
  current_rev: number;
  document: ReportDocumentView;
  computed: ComputedDocumentMeta;
}

function isBlock(value: unknown): value is Block {
  return isRecord(value) && typeof value.type === "string";
}

function isCoverConfig(value: unknown): value is CoverConfig {
  return isRecord(value);
}

function isTocConfig(value: unknown): value is TocConfig {
  return isRecord(value);
}

function isThemeConfig(value: unknown): value is ThemeConfig {
  return isRecord(value);
}

function isReferencesConfig(value: unknown): value is ReferencesConfig {
  return isRecord(value);
}

function isSourceMap(value: unknown): value is Record<string, SourceEntry> {
  return isRecord(value);
}

function isComputedDocumentMeta(value: unknown): value is ComputedDocumentMeta {
  return (
    isRecord(value) &&
    Array.isArray(value.bibliography) &&
    isRecord(value.block_framing) &&
    isRecord(value.citation_map) &&
    isRecord(value.table_layouts) &&
    Array.isArray(value.toc_tree)
  );
}

function normalizeBlock(block: Block, index: number): ReportBlock {
  const id =
    typeof block.id === "string" && block.id.length > 0
      ? block.id
      : `block-${index + 1}`;
  return { ...block, id };
}

/**
 * Normalize the flattened `GET /api/docs/{doc_id}` payload into the shape the
 * viewer expects. The server serializes `ReportDocument` with
 * `#[serde(flatten)]`, so there is no nested `document` object on the wire.
 */
export function normalizeDocumentResponse(raw: unknown): DocumentResponse {
  if (!isRecord(raw) || typeof raw.doc_id !== "string") {
    throw new Error("Malformed document response");
  }

  const revision =
    typeof raw.revision === "number"
      ? raw.revision
      : typeof raw.current_rev === "number"
        ? raw.current_rev
        : 0;

  const document: ReportDocumentView = {
    doc_id: raw.doc_id,
    session_id: typeof raw.session_id === "string" ? raw.session_id : null,
    revision,
    blocks: Array.isArray(raw.blocks)
      ? raw.blocks.filter(isBlock).map(normalizeBlock)
      : [],
    cover: isCoverConfig(raw.cover) ? raw.cover : {},
    toc: isTocConfig(raw.toc) ? raw.toc : {},
    theme: isThemeConfig(raw.theme) ? raw.theme : {},
    references: isReferencesConfig(raw.references) ? raw.references : {},
    lint: isRecord(raw.lint) ? raw.lint : {},
    sources: isSourceMap(raw.sources) ? raw.sources : undefined,
  };

  const computed: ComputedDocumentMeta = isComputedDocumentMeta(raw.computed)
    ? raw.computed
    : {
        bibliography: [],
        block_framing: {},
        citation_map: {},
        table_layouts: {},
        toc_tree: [],
      };

  return {
    current_rev:
      typeof raw.current_rev === "number" ? raw.current_rev : revision,
    document,
    computed,
  };
}

function isDocumentSyncDelta(raw: unknown): raw is DocumentSyncDelta {
  if (
    !isRecord(raw) ||
    typeof raw.doc_id !== "string" ||
    typeof raw.from_revision !== "number" ||
    typeof raw.to_revision !== "number" ||
    !isStringArray(raw.block_ids) ||
    !isComputedDocumentMeta(raw.computed)
  ) {
    return false;
  }
  if (
    [raw.cover, raw.toc, raw.theme, raw.references, raw.lint, raw.sources].some(
      (value) => value !== undefined && value !== null && !isRecord(value),
    )
  ) {
    return false;
  }
  return !(
    (raw.upserted_blocks !== undefined &&
      (!Array.isArray(raw.upserted_blocks) ||
        !raw.upserted_blocks.every(isBlock))) ||
    (raw.removed_block_ids !== undefined &&
      !isStringArray(raw.removed_block_ids))
  );
}

/** Validate the applied snapshot delta returned by `GET /api/docs/{id}/sync`. */
export function normalizeDocumentSyncDelta(raw: unknown): DocumentSyncDelta {
  if (!isDocumentSyncDelta(raw)) {
    throw new Error("Malformed document sync response");
  }
  return raw;
}

/**
 * Reuse unchanged JSON branches so React memo boundaries survive a fresh
 * server projection. Inputs are JSON values and therefore acyclic. Reused
 * branches are materialized into the fresh `next` wire value; `previous` is
 * never mutated.
 */
function reuseStructural<T>(previous: T, next: T): T {
  if (Object.is(previous, next)) {
    return previous;
  }
  if (Array.isArray(previous) && Array.isArray(next)) {
    let unchanged = previous.length === next.length;
    for (let index = 0; index < next.length; index += 1) {
      const nextValue = reuseStructural(previous[index], next[index]);
      next[index] = nextValue;
      unchanged &&= Object.is(nextValue, previous[index]);
    }
    return unchanged ? previous : next;
  }
  if (isRecord(previous) && isRecord(next)) {
    const previousKeys = Object.keys(previous);
    const nextKeys = Object.keys(next);
    let unchanged = previousKeys.length === nextKeys.length;
    for (const [key, value] of Object.entries(next)) {
      const nextValue = reuseStructural(previous[key], value);
      Reflect.set(next, key, nextValue);
      unchanged &&=
        Object.hasOwn(previous, key) && Object.is(nextValue, previous[key]);
    }
    return unchanged ? previous : next;
  }
  return next;
}

function sameBlockIdentity(
  previous: readonly ReportBlock[],
  next: readonly ReportBlock[],
): boolean {
  return (
    previous.length === next.length &&
    previous.every((block, index) => block === next[index])
  );
}

/**
 * Apply a canonical server snapshot delta while preserving every unchanged
 * block and computed branch. Any local/remote revision mismatch fails closed;
 * the caller must recover with a full document load instead of applying a
 * partial patch to the wrong base.
 */
export function applyDocumentSync(
  current: DocumentResponse,
  delta: DocumentSyncDelta,
): DocumentResponse {
  if (delta.doc_id !== current.document.doc_id) {
    throw new Error("Document sync response belongs to another document");
  }
  if (delta.to_revision === current.current_rev) {
    return current;
  }
  if (
    delta.from_revision !== current.current_rev ||
    delta.to_revision < delta.from_revision
  ) {
    throw new Error("Document sync response has a stale base revision");
  }

  const upserted = new Map<string, ReportBlock>();
  for (const [index, block] of (delta.upserted_blocks ?? []).entries()) {
    const normalized = normalizeBlock(block, index);
    if (upserted.has(normalized.id)) {
      throw new Error("Document sync response contains duplicate upserts");
    }
    upserted.set(normalized.id, normalized);
  }
  const previousById = new Map(
    current.document.blocks.map((block) => [block.id, block]),
  );
  const seen = new Set<string>();
  const nextBlocks = delta.block_ids.map((id) => {
    if (seen.has(id)) {
      throw new Error("Document sync response contains duplicate block IDs");
    }
    seen.add(id);
    const block = upserted.get(id) ?? previousById.get(id);
    if (!block) {
      throw new Error(`Document sync response is missing block ${id}`);
    }
    upserted.delete(id);
    return block;
  });
  if (upserted.size > 0) {
    throw new Error("Document sync response contains unreferenced upserts");
  }

  const blocks = sameBlockIdentity(current.document.blocks, nextBlocks)
    ? current.document.blocks
    : nextBlocks;
  const next: DocumentResponse = {
    current_rev: delta.to_revision,
    document: {
      ...current.document,
      revision: delta.to_revision,
      blocks,
      cover: isCoverConfig(delta.cover) ? delta.cover : current.document.cover,
      toc: isTocConfig(delta.toc) ? delta.toc : current.document.toc,
      theme: isThemeConfig(delta.theme) ? delta.theme : current.document.theme,
      references: isReferencesConfig(delta.references)
        ? delta.references
        : current.document.references,
      lint: isRecord(delta.lint) ? delta.lint : current.document.lint,
      sources:
        delta.sources === undefined
          ? current.document.sources
          : isSourceMap(delta.sources)
            ? delta.sources
            : current.document.sources,
    },
    computed: reuseStructural(current.computed, delta.computed),
  };
  return next;
}

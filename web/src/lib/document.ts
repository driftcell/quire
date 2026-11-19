import type {
  Block,
  ComputedDocumentMeta,
  CoverConfig,
  ReferencesConfig,
  ReportDocument,
  SourceEntry,
  ThemeConfig,
  TocConfig,
} from "@/contracts/report-protocol";
import { isRecord } from "@/lib/guards";

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
export type EchartsBlock = Omit<
  Extract<Block, { type: "echarts" }>,
  "type"
> & { type?: "echarts" };

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

import { describe, expect, test } from "bun:test";
import type { DocumentSyncDelta } from "@/contracts/report-protocol";
import {
  applyDocumentSync,
  normalizeDocumentSyncDelta,
  type DocumentResponse,
  type ReportBlock,
} from "@/lib/document";

function paragraph(id: string, text: string): ReportBlock {
  return { id, type: "paragraph", text };
}

function fixture(): DocumentResponse {
  const blocks = [
    paragraph("a", "Alpha"),
    paragraph("b", "Before"),
    paragraph("c", "Gamma"),
  ];
  return {
    current_rev: 10,
    document: {
      doc_id: "doc-1",
      session_id: "session-1",
      revision: 10,
      blocks,
      cover: { enabled: true, title: "Report" },
      toc: { enabled: true },
      theme: { preset: "default" },
      references: { enabled: true },
      lint: { ignore: [] },
      sources: {},
    },
    computed: {
      table_layouts: {},
      citation_map: {
        source1: {
          number: 1,
          label: "1",
          source_id: "source1",
          title: "Source",
        },
      },
      bibliography: [],
      toc_tree: [],
      block_framing: {},
    },
  };
}

function delta(from: number, to: number): DocumentSyncDelta {
  return {
    doc_id: "doc-1",
    from_revision: from,
    to_revision: to,
    block_ids: ["a", "b", "c"],
    computed: structuredClone(fixture().computed),
  };
}

describe("applyDocumentSync", () => {
  test("preserves unchanged blocks and computed branches", () => {
    const current = fixture();
    const nextDelta = delta(10, 12);
    nextDelta.block_ids = ["a", "b", "c"];
    nextDelta.upserted_blocks = [paragraph("b", "After")];
    nextDelta.computed = structuredClone(current.computed);
    nextDelta.computed.toc_tree = [
      { block_id: "a", text: "Alpha", level: 1, number: "1", children: [] },
    ];

    const next = applyDocumentSync(current, nextDelta);
    expect(next.current_rev).toBe(12);
    expect(next.document.revision).toBe(12);
    expect(next.document.blocks.map((block) => block.id)).toEqual([
      "a",
      "b",
      "c",
    ]);
    expect(next.document.blocks[0]).toBe(current.document.blocks[0]);
    expect(next.document.blocks[1]).not.toBe(current.document.blocks[1]);
    expect(next.document.blocks[2]).toBe(current.document.blocks[2]);
    expect(next.computed.citation_map).toBe(current.computed.citation_map);
    expect(next.computed.toc_tree).not.toBe(current.computed.toc_tree);
    expect(next.document.cover).toBe(current.document.cover);
    expect(next.document.theme).toBe(current.document.theme);
  });

  test("keeps the original block array and computed object for metadata-only revisions", () => {
    const current = fixture();
    const nextDelta = delta(10, 11);
    nextDelta.computed = structuredClone(current.computed);
    nextDelta.cover = { enabled: true, title: "Renamed" };

    const next = applyDocumentSync(current, nextDelta);
    expect(next.document.blocks).toBe(current.document.blocks);
    expect(next.computed).toBe(current.computed);
    expect(next.document.cover.title).toBe("Renamed");
    expect(next.document.toc).toBe(current.document.toc);
  });

  test("uses exact target order and omits removed blocks", () => {
    const current = fixture();
    const nextDelta = delta(10, 11);
    nextDelta.block_ids = ["c", "a"];
    nextDelta.removed_block_ids = ["b"];

    const next = applyDocumentSync(current, nextDelta);
    expect(next.document.blocks.map((block) => block.id)).toEqual(["c", "a"]);
    expect(next.document.blocks[0]).toBe(current.document.blocks[2]);
    expect(next.document.blocks[1]).toBe(current.document.blocks[0]);
  });

  test("rejects malformed optional configuration before it reaches the cache", () => {
    expect(() =>
      normalizeDocumentSyncDelta({ ...delta(10, 11), sources: "invalid" }),
    ).toThrow("Malformed document sync response");
  });

  test("fails closed on stale bases and missing target blocks", () => {
    const current = fixture();
    expect(() => applyDocumentSync(current, delta(9, 11))).toThrow(
      "stale base revision",
    );
    const missing = delta(10, 11);
    missing.block_ids = ["a", "missing"];
    expect(() => applyDocumentSync(current, missing)).toThrow("missing block");
  });
});

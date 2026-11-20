import { describe, expect, test } from "bun:test";
import type {
  EphemeralPreviewEvent,
  PreviewAnchor,
} from "@/contracts/report-protocol";
import { buildMixedItems, type MixedRenderItem } from "./ephemeralPreviewMix";
import type { PageSlice } from "./pagination";

function slice(id: string): PageSlice {
  return {
    block: { id, type: "paragraph", text: id },
  };
}

function preview(
  previewId: string,
  blockId: string,
  anchor: PreviewAnchor,
): EphemeralPreviewEvent {
  return {
    actor: "agent",
    anchor,
    block: {
      block_id: blockId,
      block_type: "paragraph",
      text: blockId,
    },
    doc_id: "doc",
    preview_id: previewId,
    received_at: new Date(0).toISOString(),
    seq: 1,
    status: "streaming",
  };
}

function itemIds(items: MixedRenderItem[]): string[] {
  return items.map((item) =>
    item.kind === "slice"
      ? item.slice.block.id
      : `preview:${item.preview.preview_id}`,
  );
}

function build(
  slices: PageSlice[],
  previews: EphemeralPreviewEvent[],
): MixedRenderItem[] {
  return buildMixedItems(
    slices,
    previews,
    slices.map((item) => item.block),
  );
}

describe("buildMixedItems preview anchors", () => {
  test("places an after_block preview immediately after the committed block", () => {
    const after = preview("p-after", "sec2_preview_after", {
      type: "after_block",
      block_id: "sec2_h1",
    });
    const items = build([slice("sec2_h1"), slice("sec2_p1")], [after]);

    expect(itemIds(items)).toEqual([
      "sec2_h1",
      "preview:p-after",
      "sec2_p1",
    ]);
  });

  test("chains after_block previews through uncommitted preview blocks", () => {
    const first = preview("p-first", "sec2_preview_first", {
      type: "after_block",
      block_id: "sec2_h1",
    });
    const second = preview("p-second", "sec2_preview_second", {
      type: "after_block",
      block_id: "sec2_preview_first",
    });
    const items = build(
      [slice("sec2_h1"), slice("sec2_p1")],
      [first, second],
    );

    expect(itemIds(items)).toEqual([
      "sec2_h1",
      "preview:p-first",
      "preview:p-second",
      "sec2_p1",
    ]);
  });

  test("chains before_block previews through uncommitted preview blocks", () => {
    const first = preview("p-first", "sec2_preview_first", {
      type: "before_block",
      block_id: "sec2_p1",
    });
    const second = preview("p-second", "sec2_preview_second", {
      type: "before_block",
      block_id: "sec2_preview_first",
    });
    const items = build(
      [slice("sec2_h1"), slice("sec2_p1")],
      [first, second],
    );

    expect(itemIds(items)).toEqual([
      "sec2_h1",
      "preview:p-second",
      "preview:p-first",
      "sec2_p1",
    ]);
  });
});

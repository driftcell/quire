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

function headingSlice(id: string, level: number): PageSlice {
  return {
    block: { id, type: "heading", level, text: id },
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

  test("replace_block keeps the streaming card at the committed block position", () => {
    const replacement = preview("p-replace", "sec1_p", {
      type: "replace_block",
      block_id: "sec1_p",
    });
    const items = build(
      [headingSlice("sec1", 1), slice("sec1_p"), headingSlice("sec2", 1)],
      [replacement],
    );

    expect(itemIds(items)).toEqual(["sec1", "preview:p-replace", "sec2"]);
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

  test("append_to_section stays after the last nested subsection block", () => {
    const appended = preview("p-section", "sec3_preview", {
      type: "append_to_section",
      section_id: "sec3_h1",
    });
    const items = build(
      [
        headingSlice("sec3_h1", 1),
        headingSlice("sec3_h3", 3),
        slice("sec3_p1"),
        headingSlice("sec4_h1", 1),
        slice("sec4_p1"),
      ],
      [appended],
    );

    expect(itemIds(items)).toEqual([
      "sec3_h1",
      "sec3_h3",
      "sec3_p1",
      "preview:p-section",
      "sec4_h1",
      "sec4_p1",
    ]);
  });

  test("hides an after_block preview until its streamed target exists", () => {
    const pending = preview("p-pending", "ch2_new", {
      type: "after_block",
      block_id: "ch2_p_update",
    });
    const items = build(
      [headingSlice("ch2", 1), slice("ch2_p1")],
      [pending],
    );

    expect(itemIds(items)).toEqual(["ch2", "ch2_p1"]);
  });

  test("hides an append_to_section preview until its section exists", () => {
    const pending = preview("p-pending", "ch2_new", {
      type: "append_to_section",
      section_id: "missing_section",
    });
    const items = build([headingSlice("ch2", 1), slice("ch2_p1")], [pending]);

    expect(itemIds(items)).toEqual(["ch2", "ch2_p1"]);
  });

  test("still renders append_to_doc previews at the document tail", () => {
    const appended = preview("p-tail", "doc_new", {
      type: "append_to_doc",
    });
    const items = build([slice("ch2_p1")], [appended]);

    expect(itemIds(items)).toEqual(["ch2_p1", "preview:p-tail"]);
  });
});

// NOTE: Only continuous mode mixes ephemeral previews; paged mode renders committed slices only.
import type { EphemeralPreviewEvent } from "@/contracts/report-protocol";
import type { PageSlice } from "@/components/report/pagination";
import type { ReportBlock } from "@/lib/document";

export type MixedRenderItem =
  | { kind: "slice"; key: string; slice: PageSlice }
  | { kind: "preview"; key: string; preview: EphemeralPreviewEvent };

export function buildMixedItems(
  slices: PageSlice[],
  previews: EphemeralPreviewEvent[],
  allBlocks: ReportBlock[],
): MixedRenderItem[] {
  if (previews.length === 0) {
    return slices.map((slice, idx) => ({
      kind: "slice",
      key: `${slice.block.id}-${slice.rowStart ?? 0}-${idx}`,
      slice,
    }));
  }

  const sectionLastBlock = new Map<string, string>();
  const openSections: Array<{ id: string; level: number }> = [];
  let previousBlockId: string | null = null;
  for (const block of allBlocks) {
    if (block.type === "heading") {
      const level = block.level ?? 1;
      while (openSections.length > 0) {
        const current = openSections.at(-1);
        if (!current || current.level < level) {
          break;
        }
        openSections.pop();
        sectionLastBlock.set(current.id, previousBlockId ?? current.id);
      }
      openSections.push({ id: block.id, level });
    }
    for (const section of openSections) {
      sectionLastBlock.set(section.id, block.id);
    }
    previousBlockId = block.id;
  }
  for (const section of openSections) {
    if (!sectionLastBlock.has(section.id)) {
      sectionLastBlock.set(section.id, previousBlockId ?? section.id);
    }
  }

  const sliceIds = new Set(slices.map((s) => s.block.id));
  const previewById = new Map<string, EphemeralPreviewEvent>();
  for (const preview of previews) {
    const blockId = preview.block.block_id;
    if (blockId) {
      previewById.set(blockId, preview);
    }
  }

  const afterMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforeMap = new Map<string, EphemeralPreviewEvent[]>();
  const replaceMap = new Map<string, EphemeralPreviewEvent>();
  const afterPreviewMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforePreviewMap = new Map<string, EphemeralPreviewEvent[]>();
  const tailPreviews: EphemeralPreviewEvent[] = [];
  const deferred = new Set<string>();

  for (const preview of previews) {
    const anchor = preview.anchor;
    if (anchor.type === "replace_block" && sliceIds.has(anchor.block_id)) {
      replaceMap.set(anchor.block_id, preview);
    } else if (anchor.type === "before_block") {
      if (sliceIds.has(anchor.block_id)) {
        const list = beforeMap.get(anchor.block_id) ?? [];
        list.push(preview);
        beforeMap.set(anchor.block_id, list);
      } else if (previewById.has(anchor.block_id)) {
        const list = beforePreviewMap.get(anchor.block_id) ?? [];
        list.push(preview);
        beforePreviewMap.set(anchor.block_id, list);
      } else {
        // The anchor field may arrive after the blocks in streamed tool arguments.
        // Keep the card hidden until the target exists instead of pinning it to the
        // document tail; the final resolved frame will render it in place.
        deferred.add(preview.preview_id);
      }
    } else if (anchor.type === "after_block") {
      if (sliceIds.has(anchor.block_id)) {
        const list = afterMap.get(anchor.block_id) ?? [];
        list.push(preview);
        afterMap.set(anchor.block_id, list);
      } else if (previewById.has(anchor.block_id)) {
        const list = afterPreviewMap.get(anchor.block_id) ?? [];
        list.push(preview);
        afterPreviewMap.set(anchor.block_id, list);
      } else {
        deferred.add(preview.preview_id);
      }
    } else if (anchor.type === "append_to_section") {
      const targetId =
        sectionLastBlock.get(anchor.section_id) ?? anchor.section_id;
      if (sliceIds.has(targetId)) {
        const list = afterMap.get(targetId) ?? [];
        list.push(preview);
        afterMap.set(targetId, list);
      } else if (previewById.has(targetId)) {
        const list = afterPreviewMap.get(targetId) ?? [];
        list.push(preview);
        afterPreviewMap.set(targetId, list);
      } else {
        deferred.add(preview.preview_id);
      }
    } else if (anchor.type === "append_to_doc") {
      tailPreviews.push(preview);
    } else {
      deferred.add(preview.preview_id);
    }
  }

  const out: MixedRenderItem[] = [];
  const emitted = new Set<string>();
  const emitPreview = (preview: EphemeralPreviewEvent) => {
    if (emitted.has(preview.preview_id)) {
      return;
    }
    emitted.add(preview.preview_id);
    const blockId = preview.block.block_id;
    if (blockId) {
      for (const child of beforePreviewMap.get(blockId) ?? []) {
        emitPreview(child);
      }
    }
    out.push({
      kind: "preview",
      key: `preview-${preview.preview_id}`,
      preview,
    });
    if (blockId) {
      for (const child of afterPreviewMap.get(blockId) ?? []) {
        emitPreview(child);
      }
    }
  };

  for (let idx = 0; idx < slices.length; idx += 1) {
    const slice = slices[idx];
    if (!slice) continue;
    const bid = slice.block.id;

    for (const preview of beforeMap.get(bid) ?? []) {
      emitPreview(preview);
    }
    const replacement = replaceMap.get(bid);
    if (replacement) {
      emitPreview(replacement);
    } else {
      out.push({
        kind: "slice",
        key: `${bid}-${slice.rowStart ?? 0}-${idx}`,
        slice,
      });
    }
    for (const preview of afterMap.get(bid) ?? []) {
      emitPreview(preview);
    }
  }
  for (const preview of tailPreviews) {
    emitPreview(preview);
  }
  for (const preview of previews) {
    if (!emitted.has(preview.preview_id) && !deferred.has(preview.preview_id)) {
      emitPreview(preview);
    }
  }
  return out;
}

function isProsePreview(
  item: MixedRenderItem,
): item is Extract<MixedRenderItem, { kind: "preview" }> {
  return (
    item.kind === "preview" &&
    (item.preview.block.block_type === "paragraph" ||
      item.preview.block.block_type === "quote")
  );
}

/** Show one author badge per consecutive same-actor prose run. */
export function shouldShowPreviewAuthor(
  items: MixedRenderItem[],
  index: number,
): boolean {
  const item = items[index];
  if (!item || item.kind !== "preview") {
    return false;
  }
  if (!isProsePreview(item)) {
    return true;
  }
  const previous = items[index - 1];
  return !(
    previous &&
    isProsePreview(previous) &&
    previous.preview.actor === item.preview.actor
  );
}

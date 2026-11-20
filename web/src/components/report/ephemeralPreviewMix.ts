import type { EphemeralPreviewEvent } from "@/contracts/report-protocol";
import type { PageSlice } from "@/components/report/pagination";
import type { ReportBlock } from "@/lib/document";

export interface MixedRenderItem {
  key: string;
  slice?: PageSlice;
  preview?: EphemeralPreviewEvent;
}

export function buildMixedItems(
  slices: PageSlice[],
  previews: EphemeralPreviewEvent[],
  allBlocks: ReportBlock[],
): MixedRenderItem[] {
  if (previews.length === 0) {
    return slices.map((slice, idx) => ({
      key: `${slice.block.id}-${slice.rowStart ?? 0}-${idx}`,
      slice,
    }));
  }

  const sectionLastBlock = new Map<string, string>();
  let activeSection: string | null = null;
  for (const b of allBlocks) {
    if (b.type === "heading") {
      activeSection = b.id;
    }
    if (activeSection) {
      sectionLastBlock.set(activeSection, b.id);
    }
  }

  const sliceIds = new Set(slices.map((s) => s.block.id));
  const afterMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforeMap = new Map<string, EphemeralPreviewEvent[]>();
  const replaceMap = new Map<string, EphemeralPreviewEvent>();
  const tailPreviews: EphemeralPreviewEvent[] = [];

  for (const p of previews) {
    const anchor = p.anchor;
    if (anchor.type === "replace_block" && sliceIds.has(anchor.block_id)) {
      replaceMap.set(anchor.block_id, p);
    } else if (
      anchor.type === "before_block" &&
      sliceIds.has(anchor.block_id)
    ) {
      const list = beforeMap.get(anchor.block_id) ?? [];
      list.push(p);
      beforeMap.set(anchor.block_id, list);
    } else if (anchor.type === "after_block" && sliceIds.has(anchor.block_id)) {
      const list = afterMap.get(anchor.block_id) ?? [];
      list.push(p);
      afterMap.set(anchor.block_id, list);
    } else if (anchor.type === "append_to_section") {
      const targetId =
        sectionLastBlock.get(anchor.section_id) ?? anchor.section_id;
      if (sliceIds.has(targetId)) {
        const list = afterMap.get(targetId) ?? [];
        list.push(p);
        afterMap.set(targetId, list);
      } else {
        tailPreviews.push(p);
      }
    } else {
      tailPreviews.push(p);
    }
  }

  const out: MixedRenderItem[] = [];
  for (let idx = 0; idx < slices.length; idx += 1) {
    const slice = slices[idx];
    if (!slice) continue;
    const bid = slice.block.id;

    for (const p of beforeMap.get(bid) ?? []) {
      out.push({ key: `pre-${p.preview_id}`, preview: p });
    }
    const replacement = replaceMap.get(bid);
    if (replacement) {
      out.push({ key: `rep-${replacement.preview_id}`, preview: replacement });
    } else {
      out.push({
        key: `${bid}-${slice.rowStart ?? 0}-${idx}`,
        slice,
      });
    }
    for (const p of afterMap.get(bid) ?? []) {
      out.push({ key: `post-${p.preview_id}`, preview: p });
    }
  }
  for (const p of tailPreviews) {
    out.push({ key: `tail-${p.preview_id}`, preview: p });
  }
  return out;
}

export function mixPreviewsIntoPages(
  pages: {
    pageNumber: number;
    items: PageSlice[];
    includeCover?: boolean;
    includeToc?: boolean;
    includeBibliography?: boolean;
  }[],
  previews: EphemeralPreviewEvent[],
  allBlocks: ReportBlock[],
): MixedRenderItem[][] {
  if (previews.length === 0) {
    return pages.map((page) =>
      page.items.map((slice, idx) => ({
        key: `${slice.block.id}-${slice.rowStart ?? 0}-${idx}`,
        slice,
      })),
    );
  }

  const sectionLastBlock = new Map<string, string>();
  let activeSection: string | null = null;
  for (const b of allBlocks) {
    if (b.type === "heading") {
      activeSection = b.id;
    }
    if (activeSection) {
      sectionLastBlock.set(activeSection, b.id);
    }
  }

  const lastOccurByBlock = new Map<
    string,
    { pageIdx: number; sliceIdx: number }
  >();
  for (let pIdx = 0; pIdx < pages.length; pIdx += 1) {
    const page = pages[pIdx];
    if (!page) continue;
    for (let sIdx = 0; sIdx < page.items.length; sIdx += 1) {
      const slice = page.items[sIdx];
      if (slice) {
        lastOccurByBlock.set(slice.block.id, { pageIdx: pIdx, sliceIdx: sIdx });
      }
    }
  }

  const afterMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforeMap = new Map<string, EphemeralPreviewEvent[]>();
  const replaceMap = new Map<string, EphemeralPreviewEvent>();
  const tailPreviews: EphemeralPreviewEvent[] = [];

  for (const p of previews) {
    const anchor = p.anchor;
    if (
      anchor.type === "replace_block" &&
      lastOccurByBlock.has(anchor.block_id)
    ) {
      replaceMap.set(anchor.block_id, p);
    } else if (
      anchor.type === "before_block" &&
      lastOccurByBlock.has(anchor.block_id)
    ) {
      const list = beforeMap.get(anchor.block_id) ?? [];
      list.push(p);
      beforeMap.set(anchor.block_id, list);
    } else if (
      anchor.type === "after_block" &&
      lastOccurByBlock.has(anchor.block_id)
    ) {
      const list = afterMap.get(anchor.block_id) ?? [];
      list.push(p);
      afterMap.set(anchor.block_id, list);
    } else if (anchor.type === "append_to_section") {
      const targetId =
        sectionLastBlock.get(anchor.section_id) ?? anchor.section_id;
      if (lastOccurByBlock.has(targetId)) {
        const list = afterMap.get(targetId) ?? [];
        list.push(p);
        afterMap.set(targetId, list);
      } else {
        tailPreviews.push(p);
      }
    } else {
      tailPreviews.push(p);
    }
  }

  const seenBefore = new Set<string>();
  const seenReplace = new Set<string>();

  return pages.map((page, pIdx) => {
    const out: MixedRenderItem[] = [];
    for (let sIdx = 0; sIdx < page.items.length; sIdx += 1) {
      const slice = page.items[sIdx];
      if (!slice) continue;
      const bid = slice.block.id;

      if (!seenBefore.has(bid)) {
        seenBefore.add(bid);
        for (const p of beforeMap.get(bid) ?? []) {
          out.push({ key: `pre-${p.preview_id}`, preview: p });
        }
      }

      const replacement = replaceMap.get(bid);
      if (replacement) {
        if (!seenReplace.has(bid)) {
          seenReplace.add(bid);
          out.push({
            key: `rep-${replacement.preview_id}`,
            preview: replacement,
          });
        }
      } else {
        out.push({
          key: `${bid}-${slice.rowStart ?? 0}-${sIdx}`,
          slice,
        });
      }

      const lastOccur = lastOccurByBlock.get(bid);
      if (
        lastOccur &&
        lastOccur.pageIdx === pIdx &&
        lastOccur.sliceIdx === sIdx
      ) {
        for (const p of afterMap.get(bid) ?? []) {
          out.push({ key: `post-${p.preview_id}`, preview: p });
        }
      }
    }

    if (pIdx === pages.length - 1) {
      for (const p of tailPreviews) {
        out.push({ key: `tail-${p.preview_id}`, preview: p });
      }
    }
    return out;
  });
}

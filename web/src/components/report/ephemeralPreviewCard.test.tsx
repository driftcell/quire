import { describe, expect, it } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { EphemeralPreviewCard } from "@/components/report/EphemeralPreviewCard";
import type { EphemeralPreviewEvent } from "@/contracts/report-protocol";

function makePreview(
  overrides: Partial<EphemeralPreviewEvent["block"]> & { actor?: string },
): EphemeralPreviewEvent {
  const { actor = "analyst", ...blockOverrides } = overrides;
  return {
    type: "ephemeral_preview",
    doc_id: "doc_1",
    preview_id: "prev_1",
    actor,
    status: "streaming",
    received_at: "2026-11-23T11:30:00Z",
    anchor: { type: "append_to_doc" },
    block: {
      block_id: "ch1_intro",
      block_type: "paragraph",
      text: "这是包含 **加粗指标** 的流式正文段落。",
      ...blockOverrides,
    },
  };
}

describe("EphemeralPreviewCard WYSIWYG rendering", () => {
  it("renders headings with report typography and no writing-status badge", () => {
    const html = renderToStaticMarkup(
      <EphemeralPreviewCard
        preview={makePreview({
          block_type: "heading",
          level: 2,
          text: "一、核心指标总览",
        })}
      />,
    );
    expect(html).toContain("<h2");
    expect(html).toContain("一、核心指标总览");
    expect(html).not.toContain("正在撰写");
    expect(html).not.toContain("正文撰写");
    expect(html).not.toContain("章节标题 H2");
  });

  it("renders paragraph inline markdown directly without placeholder wrapper", () => {
    const html = renderToStaticMarkup(
      <EphemeralPreviewCard preview={makePreview({})} />,
    );
    expect(html).toContain("<p");
    expect(html).toContain("<strong>加粗指标</strong>");
    expect(html).not.toContain("正文撰写");
  });

  it("renders left-margin AgentAvatar trigger with actor attribution", () => {
    const html = renderToStaticMarkup(
      <EphemeralPreviewCard preview={makePreview({ actor: "analyst" })} />,
    );
    expect(html).toContain('data-slot="agent-avatar"');
    expect(html).toContain('data-actor="analyst"');
    expect(html).toContain('aria-label="analyst 正在编写正文段落"');
    expect(html).not.toContain('data-slot="hover-card"');
  });

  it("can hide subsequent author badges in a consecutive prose run", () => {
    const html = renderToStaticMarkup(
      <EphemeralPreviewCard preview={makePreview({})} showAuthor={false} />,
    );
    expect(html).not.toContain('data-slot="report-author-badge"');
    expect(html).not.toContain('data-slot="agent-avatar"');
  });

  it("renders streaming table without a skeleton loading row", () => {
    const html = renderToStaticMarkup(
      <EphemeralPreviewCard
        preview={makePreview({
          block_type: "table",
          title: "表 1 营收对比",
          text: "年份 | 营收\n2025 | 120 亿元",
        })}
      />,
    );
    expect(html).toContain("<table");
    expect(html).toContain("table-fixed");
    expect(html).toContain("text-sm");
    expect(html).toContain("bg-slate-100/80");
    expect(html).toContain("表 1 营收对比");
    expect(html).toContain("120 亿元");
    expect(html).not.toContain("text-left text-[12.5px]");
    expect(html).not.toContain("加载中");
  });
});

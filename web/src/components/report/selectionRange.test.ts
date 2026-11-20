import { beforeAll, describe, expect, test } from "bun:test";
import { Window } from "happy-dom";
import {
  buildAnchorSnapshot,
  findBlockElement,
  locateQuoteRange,
  normalizeBlockIds,
  placeholderForBlock,
  quoteSelectorFromText,
  textWithinBlock,
  visibleBlockElements,
} from "./selectionRange";
import { commentThreadBlockIds } from "./reportSelection";

const domWindow = new Window();
(globalThis as { HTMLElement: unknown }).HTMLElement = domWindow.HTMLElement;
(globalThis as { Node: unknown }).Node = domWindow.Node;
// happy-dom's DOM classes are runtime-compatible but not assignable to lib.dom types.
// oxlint-disable-next-line typescript/no-unsafe-type-assertion
const domDocument = domWindow.document as unknown as Document;

beforeAll(() => {
  domDocument.body.innerHTML = `
    <div data-measure-root="true">
      <div data-block-id="hidden" data-block-kind="paragraph">Hidden</div>
    </div>
    <div data-block-id="p1" data-block-kind="paragraph">Alpha <strong>Beta</strong> Gamma Beta</div>
    <figure data-block-id="table1" data-block-kind="table">
      <figcaption>表 1 指标</figcaption>
      <table><tbody><tr><td>营收</td></tr></tbody></table>
    </figure>
    <figure data-block-id="chart1" data-block-kind="echarts">
      <figcaption>图 1 趋势</figcaption>
      <div data-chart-canvas="true"></div>
    </figure>
  `;
});

describe("selectionRange", () => {
  test("builds a contextual quote selector and trims whitespace", () => {
    const text = "前文 Alpha Beta Gamma 后文";
    const start = text.indexOf(" Beta ");
    const selector = quoteSelectorFromText(text, start, start + 6);
    expect(selector?.exact).toBe("Beta");
    expect(selector?.prefix).toContain("前文 Alpha");
    expect(selector?.suffix).toContain("Gamma 后文");
  });

  test("joins media placeholders into a bounded snapshot", () => {
    expect(
      buildAnchorSnapshot(["  Revenue  ", "[表格: 指标]", "", "[图表: 趋势]"]),
    ).toBe("Revenue … [表格: 指标] … [图表: 趋势]");
    const long = buildAnchorSnapshot(["x".repeat(3_000)]);
    expect(long.length).toBeLessThanOrEqual(2_000);
    expect(long.endsWith("…")).toBe(true);
  });

  test("normalizes and de-duplicates block ids in document order", () => {
    expect(normalizeBlockIds([" p1 ", "table1", "p1", ""])).toEqual([
      "p1",
      "table1",
    ]);
  });

  test("locates a quote using prefix disambiguation", () => {
    const block = domDocument.querySelector<HTMLElement>(
      '[data-block-id="p1"]',
    );
    expect(block).toBeTruthy();
    if (!block) return;
    const range = locateQuoteRange(block, {
      exact: "Beta",
      prefix: "Gamma ",
    });
    expect(range?.toString()).toBe("Beta");
  });

  test("computes selected text within a block without textContent.indexOf", () => {
    const block = domDocument.querySelector<HTMLElement>(
      '[data-block-id="p1"]',
    );
    expect(block).toBeTruthy();
    if (!block) return;
    const walker = domDocument.createTreeWalker(block, 4);
    const first = walker.nextNode();
    expect(first).toBeTruthy();
    if (!first) return;
    const range = domDocument.createRange();
    range.setStart(first, 0);
    range.setEnd(first, 5);
    const selected = textWithinBlock(block, range);
    expect(selected?.text).toBe("Alpha");
    expect(selected?.start).toBe(0);
    expect(selected?.end).toBe(5);
  });

  test("finds visible blocks and ignores measurement DOM", () => {
    const blocks = visibleBlockElements(domDocument.body);
    expect(blocks.map((block) => block.dataset.blockId)).toEqual([
      "p1",
      "table1",
      "chart1",
    ]);
    const nested = domDocument.querySelector("strong");
    expect(findBlockElement(nested)?.dataset.blockId).toBe("p1");
  });

  test("renders stable placeholders for table and chart blocks", () => {
    const table = domDocument.querySelector<HTMLElement>(
      '[data-block-id="table1"]',
    );
    const chart = domDocument.querySelector<HTMLElement>(
      '[data-block-id="chart1"]',
    );
    expect(table && placeholderForBlock(table)).toBe("[表格: 指标]");
    expect(chart && placeholderForBlock(chart)).toBe("[图表: 趋势]");
  });

  test("falls back to start/end blocks for legacy comment threads", () => {
    expect(
      commentThreadBlockIds({
        block_id: "p1",
        end_block_id: "chart1",
        anchor_blocks: [],
      }),
    ).toEqual(["p1", "chart1"]);
    expect(
      commentThreadBlockIds({
        block_id: "p1",
        end_block_id: "chart1",
        anchor_blocks: ["p1", "table1", "chart1"],
      }),
    ).toEqual(["p1", "table1", "chart1"]);
  });
});

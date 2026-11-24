import { describe, expect, test } from "bun:test";
import { normalizeChatMl } from "@/components/chat/chatRichContext";

describe("normalizeChatMl short ChatML protocol", () => {
  test("extracts self-closing and paired <citation> tags in appearance order and deduplicates", () => {
    const input =
      '浙江大学2025年国自然立项表现突出<citation id="nsfc_2025"/>，其中青年项目增长显著<citation id="zju_rep">详见年报</citation>，且与前期统计一致<citation id="nsfc_2025"/>。';
    const result = normalizeChatMl(input);
    expect(result.citedSourceIds).toEqual(["nsfc_2025", "zju_rep"]);
    expect(result.markdown).toContain('<citation id="nsfc_2025"></citation>');
    expect(result.markdown).toContain('<citation id="zju_rep"></citation>');
  });

  test("converts markdown footnote citations [^id] into short ChatML citation tags", () => {
    const input = "经费总额达到35.2亿元[^moe_stat_25]。";
    const result = normalizeChatMl(input);
    expect(result.citedSourceIds).toEqual(["moe_stat_25"]);
    expect(result.markdown).toContain('<citation id="moe_stat_25"></citation>');
  });

  test("extracts <echarts id='...'/> tags into block-level paired echarts elements", () => {
    const input =
      '核心趋势如下：\n<echarts id="ch2_funding_chart"/>\n以上为近五年趋势。';
    const result = normalizeChatMl(input);
    expect(result.chartIds).toEqual(["ch2_funding_chart"]);
    expect(result.markdown).toContain(
      '<echarts id="ch2_funding_chart"></echarts>',
    );
  });

  test("strips incomplete trailing ChatML tag during streaming without corrupting preceding text", () => {
    const input = "正在输出结论<citation id=\"nsfc_20";
    const result = normalizeChatMl(input);
    expect(result.markdown).toBe("正在输出结论");
    expect(result.citedSourceIds).toEqual([]);
  });
});

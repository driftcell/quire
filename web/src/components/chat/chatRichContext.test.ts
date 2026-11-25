import { describe, expect, test } from "bun:test";
import {
  buildChatRichContext,
  normalizeXml,
} from "@/components/chat/chatRichContext";
import { resolveShortCitationLabel } from "@/components/ui/inline-citation";

describe("normalizeXml short XML protocol", () => {
  test("extracts self-closing and paired <citation> tags in appearance order and deduplicates", () => {
    const input =
      '浙江大学2025年国自然立项表现突出<citation id="nsfc_2025"/>，其中青年项目增长显著<citation id="zju_rep">详见年报</citation>，且与前期统计一致<citation id="nsfc_2025"/>。';
    const result = normalizeXml(input);
    expect(result.citedSourceIds).toEqual(["nsfc_2025", "zju_rep"]);
    expect(result.markdown).toContain('<citation id="nsfc_2025"></citation>');
    expect(result.markdown).toContain('<citation id="zju_rep"></citation>');
  });

  test("converts markdown footnote citations [^id] into short XML citation tags", () => {
    const input = "经费总额达到35.2亿元[^moe_stat_25]。";
    const result = normalizeXml(input);
    expect(result.citedSourceIds).toEqual(["moe_stat_25"]);
    expect(result.markdown).toContain('<citation id="moe_stat_25"></citation>');
  });

  test("extracts <echarts id='...'/> tags into block-level paired echarts elements", () => {
    const input =
      '核心趋势如下：\n<echarts id="ch2_funding_chart"/>\n以上为近五年趋势。';
    const result = normalizeXml(input);
    expect(result.chartIds).toEqual(["ch2_funding_chart"]);
    expect(result.markdown).toContain(
      '<echarts id="ch2_funding_chart"></echarts>',
    );
  });

  test("strips incomplete trailing XML tag during streaming without corrupting preceding text", () => {
    const input = '正在输出结论<citation id="nsfc_20';
    const result = normalizeXml(input);
    expect(result.markdown).toBe("正在输出结论");
    expect(result.citedSourceIds).toEqual([]);
  });

  test("ignores literal placeholder IDs and citation syntax inside inline code or fenced code blocks", () => {
    const input =
      '真实数据结论<citation id="zstu_official"/>，占位示例 `[^source_id]` 与 `<citation id="source_id"/>` 以及裸占位 [^source_id] 不应计入引用。';
    const result = normalizeXml(input);
    expect(result.citedSourceIds).toEqual(["zstu_official"]);
    expect(result.markdown).toContain(
      '<citation id="zstu_official"></citation>',
    );
    expect(result.markdown).toContain("`[^source_id]`");
    expect(result.markdown).toContain('`<citation id="source_id"/>`');
    expect(result.markdown).not.toContain(
      '<citation id="source_id"></citation>',
    );
  });

  test("extracts sources from apply_operations object maps and resolves Mapped canonical aliases", () => {
    const ctx = buildChatRichContext(
      null,
      [
        {
          run_id: "run_1",
          status: "completed",
          segments: [
            {
              kind: "execution",
              tools: [
                {
                  tool_call_id: "tc_1",
                  tool_name: "apply_operations",
                  status: "completed",
                  input_preview: JSON.stringify({
                    sources: {
                      zstu_25: {
                        title: "浙江理工大学2025年科研统计公报",
                        publisher: "浙江理工大学官网",
                        url: "https://www.zstu.edu.cn/report",
                      },
                    },
                  }),
                },
                {
                  tool_call_id: "tc_2",
                  tool_name: "upsert_sources",
                  status: "completed",
                  input_preview: JSON.stringify({
                    sources: [
                      {
                        id: "zstu_dup",
                        title: "浙江理工大学2025年科研统计公报",
                        publisher: "浙江理工大学官网",
                      },
                    ],
                  }),
                  output_preview: JSON.stringify({
                    text: "OK — doc rev=2\nMapped: zstu_dup -> zstu_25",
                  }),
                },
              ],
            },
          ],
        },
      ],
      null,
      [],
    );

    const resolved = ctx.sources.get("zstu_25");
    expect(resolved?.publisher).toBe("浙江理工大学官网");
    expect(resolveShortCitationLabel(resolved, "zstu_25")).toBe(
      "浙江理工大学官网",
    );
    expect(ctx.sources.get("zstu_dup")?.publisher).toBe("浙江理工大学官网");
  });
});

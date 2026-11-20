import { describe, expect, test } from "bun:test";
import {
  editableCommentRefText,
  parseCommentRefs,
  serializeCommentRefs,
  updateCommentRefContent,
  type CommentRefPayload,
} from "./commentRefXml";

const ref = (
  overrides: Partial<CommentRefPayload> = {},
): CommentRefPayload => ({
  thread_id: "ct_1",
  block_id: "sec2_p3",
  status: "open",
  comment: "请补充同比口径。",
  ...overrides,
});

describe("commentRefXml", () => {
  test("round-trips one reference with a quote and user text", () => {
    const payload = serializeCommentRefs(
      [ref({ quote: "营业收入同比增长 12.4%" })],
      "请按批注修订。",
    );
    expect(payload).toContain('thread_id="ct_1"');
    expect(payload).toContain("<quote>营业收入同比增长 12.4%</quote>");

    const parsed = parseCommentRefs(payload);
    expect(parsed.refs).toEqual([
      {
        thread_id: "ct_1",
        block_id: "sec2_p3",
        status: "open",
        quote: "营业收入同比增长 12.4%",
        comment: "请补充同比口径。",
      },
    ]);
    expect(parsed.body).toBe("请按批注修订。");
  });

  test("round-trips multiple references", () => {
    const parsed = parseCommentRefs(
      serializeCommentRefs(
        [
          ref({ thread_id: "ct_1", block_id: "sec1" }),
          ref({
            thread_id: "ct_2",
            block_id: "sec2_p3",
            status: "addressed",
            quote: "毛利率",
          }),
        ],
        "一起处理",
      ),
    );
    expect(parsed.refs).toHaveLength(2);
    expect(parsed.refs[1]).toMatchObject({
      thread_id: "ct_2",
      block_id: "sec2_p3",
      status: "addressed",
      quote: "毛利率",
    });
    expect(parsed.body).toBe("一起处理");
  });

  test("round-trips cross-block ranges and end quotes", () => {
    const payload = serializeCommentRefs(
      [
        ref({
          block_id: "sec1",
          end_block_id: "chart1",
          anchor_blocks: ["sec1", "table1", "chart1"],
          quote: "营收增长",
          end_quote: "趋势回落",
          anchor_snapshot: "营收增长 … [表格: 指标] … [图表: 趋势]",
        }),
      ],
      "请对照整段修改",
    );
    expect(payload).toContain('end_block_id="chart1"');
    expect(payload).toContain('anchor_blocks="sec1,table1,chart1"');
    expect(payload).toContain("<end_quote>趋势回落</end_quote>");
    expect(payload).toContain(
      "<anchor_snapshot>营收增长 … [表格: 指标] … [图表: 趋势]</anchor_snapshot>",
    );
    const parsed = parseCommentRefs(payload);
    expect(parsed.refs[0]).toMatchObject({
      block_id: "sec1",
      end_block_id: "chart1",
      anchor_blocks: ["sec1", "table1", "chart1"],
      quote: "营收增长",
      end_quote: "趋势回落",
      anchor_snapshot: "营收增长 … [表格: 指标] … [图表: 趋势]",
    });
    expect(parsed.body).toBe("请对照整段修改");
  });

  test("parses legacy single-block references without new fields", () => {
    const parsed = parseCommentRefs(
      '<quire_comment_ref thread_id="ct_old" block_id="sec1" status="open"><quote>旧文</quote><comment>旧批注</comment></quire_comment_ref>',
    );
    expect(parsed.refs[0]).toEqual({
      thread_id: "ct_old",
      block_id: "sec1",
      status: "open",
      quote: "旧文",
      comment: "旧批注",
    });
  });

  test("omits the quote element when no quote is provided", () => {
    const parsed = parseCommentRefs(
      serializeCommentRefs([ref({ quote: undefined })], ""),
    );
    expect(parsed.refs).toHaveLength(1);
    expect(parsed.refs[0]?.quote).toBeUndefined();
    expect(parsed.body).toBe("");
  });

  test("escapes and restores XML special characters", () => {
    const parsed = parseCommentRefs(
      serializeCommentRefs(
        [
          ref({
            quote: 'A < B & C > D "quoted"',
            comment: '5 < 7 & 8 > 3 <tag> "ok"',
          }),
        ],
        '用户文本 <保留> & "引号"',
      ),
    );
    expect(parsed.refs[0]?.quote).toBe('A < B & C > D "quoted"');
    expect(parsed.refs[0]?.comment).toBe('5 < 7 & 8 > 3 <tag> "ok"');
    expect(parsed.body).toBe('用户文本 <保留> & "引号"');
  });

  test("leaves ordinary text untouched", () => {
    const content = "普通消息，没有 XML。\n第二行仍然保留。";
    const parsed = parseCommentRefs(content);
    expect(parsed.refs).toEqual([]);
    expect(parsed.body).toBe(content);
  });

  test("degrades malformed XML to original text without throwing", () => {
    const malformed = [
      '<quire_comment_ref thread_id="ct_1" block_id="sec1">',
      "<quote>缺少关闭标签",
      "<comment>正文</comment>",
      "</quire_comment_ref>",
    ].join("\n");
    expect(() => parseCommentRefs(malformed)).not.toThrow();
    const parsed = parseCommentRefs(malformed);
    expect(parsed.refs).toEqual([]);
    expect(parsed.body).toBe(malformed);
  });

  test("rejects a malformed reference instead of partially parsing a batch", () => {
    const content = [
      serializeCommentRefs([ref()], ""),
      '<quire_comment_ref thread_id="ct_2" block_id="sec2">',
      "<comment>没有结束标签</comment>",
    ].join("\n");
    const parsed = parseCommentRefs(content);
    expect(parsed.refs).toEqual([]);
    expect(parsed.body).toBe(content);
  });

  test("edits comment-only payloads without dropping the XML structure", () => {
    const original = serializeCommentRefs([ref({ comment: "旧批注" })], "");
    expect(editableCommentRefText(original)).toBe("旧批注");
    const updated = updateCommentRefContent(original, "新批注");
    const parsed = parseCommentRefs(updated);
    expect(parsed.refs).toHaveLength(1);
    expect(parsed.refs[0]?.comment).toBe("新批注");
    expect(parsed.refs[0]?.thread_id).toBe("ct_1");
    expect(parsed.body).toBe("");
  });

  test("edits the trailing instruction while preserving refs", () => {
    const original = serializeCommentRefs(
      [ref({ quote: "原文" })],
      "请修改这一段",
    );
    expect(editableCommentRefText(original)).toBe("请修改这一段");
    const updated = updateCommentRefContent(original, "改成更简洁的表达");
    const parsed = parseCommentRefs(updated);
    expect(parsed.refs[0]?.comment).toBe("请补充同比口径。");
    expect(parsed.body).toBe("改成更简洁的表达");
  });
});

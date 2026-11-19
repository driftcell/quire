import { describe, expect, test } from "bun:test";
import {
  parseCommentRefs,
  serializeCommentRefs,
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
});

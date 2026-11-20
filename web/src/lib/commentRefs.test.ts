import { describe, expect, test } from "bun:test";
import type { SessionCommentRef } from "@/contracts/session-protocol";
import {
  pendingMessageDraftForEdit,
  updatePendingMessageDraft,
} from "./commentRefs";

function ref(
  threadId: string,
  comment: string,
  overrides: Partial<SessionCommentRef> = {},
): SessionCommentRef {
  return {
    thread_id: threadId,
    block_id: `block-${threadId}`,
    end_block_id: null,
    anchor_blocks: [],
    status: "open",
    quote: null,
    end_quote: null,
    anchor_snapshot: null,
    comment,
    ...overrides,
  };
}

describe("structured pending comment drafts", () => {
  test("comment-only messages edit comments rather than manufacturing body content", () => {
    const refs = [ref("ct_1", "旧批注")];
    expect(pendingMessageDraftForEdit("", refs)).toBe("旧批注");

    const updated = updatePendingMessageDraft("", refs, "新批注");
    expect(updated.content).toBe("");
    expect(updated.comment_refs.map((item) => item.comment)).toEqual([
      "新批注",
    ]);
    expect(updated.comment_refs[0]?.thread_id).toBe("ct_1");
  });

  test("paragraph edits update corresponding references in order", () => {
    const refs = [ref("ct_1", "旧一"), ref("ct_2", "旧二")];
    expect(pendingMessageDraftForEdit("", refs)).toBe("旧一\n\n旧二");

    const updated = updatePendingMessageDraft("", refs, "新一\n\n新二");
    expect(updated.content).toBe("");
    expect(updated.comment_refs.map((item) => item.comment)).toEqual([
      "新一",
      "新二",
    ]);
  });

  test("messages with an instruction keep references unchanged", () => {
    const refs = [ref("ct_1", "批注")];
    expect(pendingMessageDraftForEdit("原指令", refs)).toBe("原指令");

    const updated = updatePendingMessageDraft("原指令", refs, "修改后的指令");
    expect(updated).toEqual({
      content: "修改后的指令",
      comment_refs: refs,
    });
  });

  test("ordinary pending messages only update content", () => {
    expect(updatePendingMessageDraft("原文", [], "  新文  ")).toEqual({
      content: "新文",
      comment_refs: [],
    });
  });
});

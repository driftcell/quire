export interface CommentRefPayload {
  thread_id: string;
  block_id: string;
  end_block_id?: string;
  anchor_blocks?: string[];
  status: string;
  quote?: string;
  end_quote?: string;
  anchor_snapshot?: string;
  comment: string;
}

export interface ParsedCommentRefs {
  refs: CommentRefPayload[];
  body: string;
}

const BLOCK_SOURCE = String.raw`<quire_comment_ref\b([^>]*)>([\s\S]*?)<\/quire_comment_ref>`;
const ATTRIBUTE_RE = /([A-Za-z_][\w-]*)\s*=\s*"([^"]*)"/g;
const COMMENT_RE = /<comment>([\s\S]*?)<\/comment>/;
const QUOTE_RE = /<quote>([\s\S]*?)<\/quote>/;
const END_QUOTE_RE = /<end_quote>([\s\S]*?)<\/end_quote>/;
const SNAPSHOT_RE = /<anchor_snapshot>([\s\S]*?)<\/anchor_snapshot>/;

function escapeXml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function unescapeXml(value: string): string {
  return value
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&apos;", "'")
    .replaceAll("&#39;", "'")
    .replaceAll("&amp;", "&");
}

function parseAttributes(source: string): Map<string, string> {
  const attributes = new Map<string, string>();
  const re = new RegExp(ATTRIBUTE_RE.source, "g");
  let match: RegExpExecArray | null = re.exec(source);
  while (match !== null) {
    const key = match[1];
    const rawValue = match[2];
    if (key && rawValue !== undefined) {
      attributes.set(key, unescapeXml(rawValue));
    }
    match = re.exec(source);
  }
  return attributes;
}

function fallback(content: string): ParsedCommentRefs {
  return { refs: [], body: content };
}

/**
 * Serialize queued comment references as XML blocks followed by the user's
 * free-form instruction. The XML sits at the top so agents can parse it before
 * reading the rest of the message.
 */
export function serializeCommentRefs(
  refs: CommentRefPayload[],
  userText: string,
): string {
  const blocks = refs.map((ref) => {
    const attributes = [
      `thread_id="${escapeXml(ref.thread_id)}"`,
      `block_id="${escapeXml(ref.block_id)}"`,
      `status="${escapeXml(ref.status)}"`,
    ];
    if (ref.end_block_id) {
      attributes.push(`end_block_id="${escapeXml(ref.end_block_id)}"`);
    }
    if (ref.anchor_blocks && ref.anchor_blocks.length > 1) {
      attributes.push(
        `anchor_blocks="${escapeXml(ref.anchor_blocks.join(","))}"`,
      );
    }
    const lines = [`<quire_comment_ref ${attributes.join(" ")}>`];
    const quote = ref.quote?.trim();
    if (quote) {
      lines.push(`  <quote>${escapeXml(quote)}</quote>`);
    }
    const endQuote = ref.end_quote?.trim();
    if (endQuote) {
      lines.push(`  <end_quote>${escapeXml(endQuote)}</end_quote>`);
    }
    const snapshot = ref.anchor_snapshot?.trim();
    if (snapshot) {
      lines.push(`  <anchor_snapshot>${escapeXml(snapshot)}</anchor_snapshot>`);
    }
    lines.push(`  <comment>${escapeXml(ref.comment)}</comment>`);
    lines.push("</quire_comment_ref>");
    return lines.join("\n");
  });
  const text = userText.trim();
  return [...blocks, text].filter(Boolean).join("\n\n");
}

/**
 * Extract all valid `<quire_comment_ref>` blocks and return the remaining
 * message body. Any malformed XML degrades to the original plain text and never
 * throws, so historical messages remain readable.
 */
export function parseCommentRefs(content: string): ParsedCommentRefs {
  if (!content || !content.includes("<quire_comment_ref")) {
    return { refs: [], body: content };
  }
  try {
    const blockRe = new RegExp(BLOCK_SOURCE, "g");
    const matches = Array.from(content.matchAll(blockRe));
    if (matches.length === 0) {
      return fallback(content);
    }

    const refs: CommentRefPayload[] = [];
    for (const match of matches) {
      const rawAttributes = match[1] ?? "";
      const inner = match[2] ?? "";
      const attributes = parseAttributes(rawAttributes);
      const threadId = attributes.get("thread_id")?.trim();
      const blockId = attributes.get("block_id")?.trim();
      if (!threadId || !blockId) {
        return fallback(content);
      }
      const commentMatch = COMMENT_RE.exec(inner);
      if (!commentMatch) {
        return fallback(content);
      }
      const comment = unescapeXml(commentMatch[1] ?? "").trim();
      if (!comment) {
        return fallback(content);
      }
      if (inner.includes("<quote>")) {
        const quoteMatch = QUOTE_RE.exec(inner);
        if (!quoteMatch) {
          return fallback(content);
        }
      }
      if (inner.includes("<end_quote>")) {
        const endQuoteMatch = END_QUOTE_RE.exec(inner);
        if (!endQuoteMatch) {
          return fallback(content);
        }
      }
      if (inner.includes("<anchor_snapshot>")) {
        const snapshotMatch = SNAPSHOT_RE.exec(inner);
        if (!snapshotMatch) {
          return fallback(content);
        }
      }
      const quoteMatch = QUOTE_RE.exec(inner);
      const quote = quoteMatch
        ? unescapeXml(quoteMatch[1] ?? "").trim()
        : undefined;
      const endQuoteMatch = END_QUOTE_RE.exec(inner);
      const endQuote = endQuoteMatch
        ? unescapeXml(endQuoteMatch[1] ?? "").trim()
        : undefined;
      const snapshotMatch = SNAPSHOT_RE.exec(inner);
      const anchorSnapshot = snapshotMatch
        ? unescapeXml(snapshotMatch[1] ?? "").trim()
        : undefined;
      const anchorBlocks = attributes
        .get("anchor_blocks")
        ?.split(",")
        .map((value) => value.trim())
        .filter(Boolean);
      const endBlockId = attributes.get("end_block_id")?.trim();
      refs.push({
        thread_id: threadId,
        block_id: blockId,
        ...(endBlockId ? { end_block_id: endBlockId } : {}),
        ...(anchorBlocks && anchorBlocks.length > 0 ? { anchor_blocks: anchorBlocks } : {}),
        status: attributes.get("status")?.trim() || "open",
        ...(quote ? { quote } : {}),
        ...(endQuote ? { end_quote: endQuote } : {}),
        ...(anchorSnapshot ? { anchor_snapshot: anchorSnapshot } : {}),
        comment,
      });
    }

    const body = content.replace(new RegExp(BLOCK_SOURCE, "g"), "").trim();
    if (
      body.includes("<quire_comment_ref") ||
      body.includes("</quire_comment_ref>")
    ) {
      return fallback(content);
    }
    return { refs, body };
  } catch {
    return fallback(content);
  }
}

/** Text shown in the pending-bubble editor for comment-ref content. */
export function editableCommentRefText(content: string): string {
  const parsed = parseCommentRefs(content);
  if (parsed.refs.length > 0 && parsed.body.trim().length === 0) {
    return parsed.refs.map((ref) => ref.comment).join("\n\n");
  }
  return parsed.body || content;
}

/**
 * Rebuild a pending queue payload after inline editing while preserving XML
 * structure. Comment-only payloads update `<comment>` bodies; payloads with a
 * user instruction keep the refs and replace only the trailing text.
 */
export function updateCommentRefContent(
  content: string,
  editedText: string,
): string {
  const parsed = parseCommentRefs(content);
  const text = editedText.trim();
  if (parsed.refs.length === 0) {
    return text;
  }
  if (parsed.body.trim().length > 0) {
    return serializeCommentRefs(parsed.refs, text);
  }
  const parts = text
    .split(/\n{2,}/)
    .map((part) => part.trim())
    .filter(Boolean);
  if (parts.length <= 1) {
    return serializeCommentRefs(
      parsed.refs.map((ref) => ({ ...ref, comment: text })),
      "",
    );
  }
  return serializeCommentRefs(
    parsed.refs.map((ref, index) => ({
      ...ref,
      comment: parts[index] ?? ref.comment,
    })),
    "",
  );
}

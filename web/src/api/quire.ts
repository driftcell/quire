import type {
  CommentStatus,
  CommentThread,
  DocumentDiffReport,
  TextQuoteSelector,
} from "@/contracts/report-protocol";
import { bearerHeaders, getAuthTokens } from "@/lib/auth";
import {
  normalizeDocumentResponse,
  type DocumentResponse,
} from "@/lib/document";
import { isRecord } from "@/lib/guards";

export type DocumentView = DocumentResponse;

function buildHeaders(
  token: string,
  initHeaders: HeadersInit | undefined,
): Headers {
  const headers = new Headers(initHeaders);
  if (!headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  for (const [key, value] of Object.entries(bearerHeaders(token))) {
    headers.set(key, value);
  }
  return headers;
}

async function quireFetch<T>(
  path: string,
  mode: "ro" | "rw",
  decode: (raw: unknown) => T,
  init: RequestInit = {},
  tokenOverride?: string,
): Promise<T> {
  const { quireRoToken, quireRwToken } = getAuthTokens();
  const token =
    tokenOverride ??
    (mode === "rw" ? quireRwToken : quireRoToken || quireRwToken);
  const response = await fetch(path, {
    ...init,
    headers: buildHeaders(token, init.headers),
  });
  if (!response.ok) {
    const text = await response.text().catch(() => response.statusText);
    throw new Error(text || `Quire HTTP ${response.status}`);
  }
  const raw: unknown = await response.json();
  return decode(raw);
}

function decodeWith<T>(
  raw: unknown,
  guard: (value: unknown) => value is T,
  label: string,
): T {
  if (!guard(raw)) {
    throw new Error(`Malformed ${label} response`);
  }
  return raw;
}

function isCommentThread(value: unknown): value is CommentThread {
  return (
    isRecord(value) &&
    typeof value.thread_id === "string" &&
    typeof value.doc_id === "string" &&
    Array.isArray(value.messages)
  );
}

function isCommentThreadList(value: unknown): value is CommentThread[] {
  return Array.isArray(value) && value.every(isCommentThread);
}

function isDocumentDiffReport(value: unknown): value is DocumentDiffReport {
  return (
    isRecord(value) &&
    typeof value.doc_id === "string" &&
    Array.isArray(value.blocks)
  );
}

export function getDocument(
  docId: string,
  tokenOverride?: string,
): Promise<DocumentView> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}`,
    "ro",
    normalizeDocumentResponse,
    {},
    tokenOverride,
  );
}

export function listComments(
  docId: string,
  status: CommentStatus | "all" = "all",
): Promise<CommentThread[]> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}/comments?status=${status}`,
    "ro",
    (raw) => decodeWith(raw, isCommentThreadList, "comment list"),
  );
}

export function createComment(
  docId: string,
  payload: {
    block_id: string;
    author: string;
    body: string;
    quote?: TextQuoteSelector;
  },
): Promise<CommentThread> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}/comments`,
    "rw",
    (raw) => decodeWith(raw, isCommentThread, "comment"),
    {
      method: "POST",
      body: JSON.stringify({
        block_id: payload.block_id,
        author_type: "human",
        author: payload.author,
        body: payload.body,
        quote: payload.quote,
      }),
    },
  );
}

export function replyComment(
  docId: string,
  threadId: string,
  payload: { author: string; body: string },
): Promise<CommentThread> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}/comments/${encodeURIComponent(threadId)}/replies`,
    "rw",
    (raw) => decodeWith(raw, isCommentThread, "comment"),
    {
      method: "POST",
      body: JSON.stringify({
        author_type: "human",
        author: payload.author,
        body: payload.body,
        mark_addressed: false,
      }),
    },
  );
}

export function updateCommentStatus(
  docId: string,
  threadId: string,
  status: CommentStatus,
): Promise<CommentThread> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}/comments/${encodeURIComponent(threadId)}`,
    "rw",
    (raw) => decodeWith(raw, isCommentThread, "comment"),
    {
      method: "PATCH",
      body: JSON.stringify({ status }),
    },
  );
}

export function getDocumentDiff(
  docId: string,
  fromRev: number,
  toRev?: number,
): Promise<DocumentDiffReport> {
  const params = new URLSearchParams({ fromRev: String(fromRev) });
  if (toRev !== undefined) {
    params.set("toRev", String(toRev));
  }
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}/diff?${params.toString()}`,
    "ro",
    (raw) => decodeWith(raw, isDocumentDiffReport, "document diff"),
  );
}

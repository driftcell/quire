import type {
  CommentStatus,
  CommentThread,
  DocumentDiffReport,
  DocumentSyncDelta,
} from "@/contracts/report-protocol";
import { bearerHeaders, clearAuthSession, getAccessToken } from "@/lib/auth";
import {
  normalizeDocumentResponse,
  normalizeDocumentSyncDelta,
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
  _mode: "ro" | "rw",
  decode: (raw: unknown) => T,
  init: RequestInit = {},
  tokenOverride?: string,
): Promise<T> {
  const token = tokenOverride ?? getAccessToken();
  const response = await fetch(path, {
    ...init,
    headers: buildHeaders(token, init.headers),
  });
  if (!response.ok) {
    if (response.status === 401 && !tokenOverride) {
      clearAuthSession();
    }
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

interface CommentListView {
  doc_id: string;
  comments: CommentThread[];
}

function isCommentListView(value: unknown): value is CommentListView {
  return (
    isRecord(value) &&
    typeof value.doc_id === "string" &&
    Array.isArray(value.comments) &&
    value.comments.every(isCommentThread)
  );
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
  revision?: number,
): Promise<DocumentView> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}${revision === undefined ? "" : `?rev=${revision}`}`,
    "ro",
    (raw) => {
      const view = normalizeDocumentResponse(raw);
      if (revision !== undefined && (view.current_rev !== revision || view.document.revision !== revision)) {
        throw new Error(`请求第 ${revision} 版，返回第 ${view.document.revision} 版，无法预览。`);
      }
      return view;
    },
    {},
    tokenOverride,
  );
}

export function getDocumentSync(
  docId: string,
  fromRev: number,
): Promise<DocumentSyncDelta> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}/sync?fromRev=${fromRev}`,
    "ro",
    normalizeDocumentSyncDelta,
  );
}

export function listComments(
  docId: string,
  status: CommentStatus | "all" = "all",
  tokenOverride?: string,
): Promise<CommentThread[]> {
  return quireFetch(
    `/api/docs/${encodeURIComponent(docId)}/comments?status=${status}`,
    "ro",
    (raw) => {
      if (Array.isArray(raw)) {
        return decodeWith(raw, isCommentThreadList, "comment list");
      }
      return decodeWith(raw, isCommentListView, "comment list").comments;
    },
    {},
    tokenOverride,
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

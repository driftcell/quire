import type {
  AttachmentMeta,
  CreateSessionRequest,
  QuestionAnswer,
  RunTraceSegment,
  RunTraceSubagent,
  RunTraceSummary,
  RunTraceToolItem,
  SendMessageRequest,
  SessionDetail,
  SessionMessage,
  SessionStatus,
  SessionSummary,
  ToolCallDetail,
} from "@/contracts/session-protocol";
import type {
  ChartDataTable,
  ChartRequirements,
} from "@/contracts/report-protocol";
import { bearerHeaders, getAuthTokens } from "@/lib/auth";
import type { EchartsBlock } from "@/lib/document";
import { isRecord } from "@/lib/guards";

export interface ChartPreviewResponse {
  preview_id: string;
  chart: EchartsBlock;
  requirements?: ChartRequirements;
  tables?: ChartDataTable[];
  title?: string | null;
  note?: string | null;
  based_on_preview_id?: string | null;
}

export class ApiError extends Error {
  status: number;
  code: string;
  hint?: string;

  constructor(status: number, code: string, message: string, hint?: string) {
    super(message);
    this.status = status;
    this.code = code;
    this.hint = hint;
  }
}

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

async function runtimeFetch<T>(
  path: string,
  decode: (raw: unknown) => T,
  init: RequestInit = {},
  tokenOverride?: string,
): Promise<T> {
  const { runtimeToken } = getAuthTokens();
  const response = await fetch(path, {
    ...init,
    headers: buildHeaders(tokenOverride ?? runtimeToken, init.headers),
  });
  if (!response.ok) {
    let code = `HTTP_${response.status}`;
    let message = response.statusText || "Request failed";
    let hint: string | undefined;
    try {
      const body: unknown = await response.json();
      if (isRecord(body)) {
        const error = isRecord(body.error) ? body.error : body;
        if (typeof error.code === "string") code = error.code;
        if (typeof error.message === "string") message = error.message;
        if (typeof error.hint === "string") hint = error.hint;
      }
    } catch {
      // Non-JSON error body.
    }
    throw new ApiError(response.status, code, message, hint);
  }
  if (response.status === 204) {
    return decode(undefined);
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

function isSessionSummary(value: unknown): value is SessionSummary {
  return (
    isRecord(value) &&
    typeof value.session_id === "string" &&
    typeof value.title === "string"
  );
}

function isSessionSummaryList(value: unknown): value is SessionSummary[] {
  return Array.isArray(value) && value.every(isSessionSummary);
}

function isSessionDetail(value: unknown): value is SessionDetail {
  return (
    isRecord(value) &&
    typeof value.session_id === "string" &&
    typeof value.title === "string"
  );
}

function isSessionMessage(value: unknown): value is SessionMessage {
  return (
    isRecord(value) &&
    typeof value.message_id === "string" &&
    typeof value.content === "string"
  );
}

function isAttachmentMeta(value: unknown): value is AttachmentMeta {
  return isRecord(value) && typeof value.attachment_id === "string";
}

function isToolCallDetail(value: unknown): value is ToolCallDetail {
  return isRecord(value) && typeof value.tool_call_id === "string";
}

function isRunTraceToolItem(value: unknown): value is RunTraceToolItem {
  return (
    isRecord(value) &&
    typeof value.tool_call_id === "string" &&
    typeof value.tool_name === "string" &&
    typeof value.status === "string" &&
    typeof value.input_preview === "string"
  );
}

function isRunTraceSubagent(value: unknown): value is RunTraceSubagent {
  return (
    isRecord(value) &&
    typeof value.subagent_run_id === "string" &&
    typeof value.agent_name === "string" &&
    typeof value.task === "string" &&
    typeof value.status === "string" &&
    typeof value.thinking === "string" &&
    (value.tools === undefined ||
      (Array.isArray(value.tools) && value.tools.every(isRunTraceToolItem)))
  );
}

function isRunTraceSegment(value: unknown): value is RunTraceSegment {
  if (!isRecord(value)) {
    return false;
  }
  if (value.kind === "thinking" || value.kind === "text") {
    return typeof value.text === "string";
  }
  if (value.kind === "execution") {
    return (
      (value.tools === undefined ||
        (Array.isArray(value.tools) &&
          value.tools.every(isRunTraceToolItem))) &&
      (value.subagents === undefined ||
        (Array.isArray(value.subagents) &&
          value.subagents.every(isRunTraceSubagent)))
    );
  }
  return false;
}

function isRunTraceSummary(value: unknown): value is RunTraceSummary {
  return (
    isRecord(value) &&
    typeof value.run_id === "string" &&
    typeof value.status === "string" &&
    (value.segments === undefined ||
      (Array.isArray(value.segments) &&
        value.segments.every(isRunTraceSegment)))
  );
}

function isRunTraceSummaryList(value: unknown): value is RunTraceSummary[] {
  return Array.isArray(value) && value.every(isRunTraceSummary);
}

export interface PostMessageResponse {
  message: SessionMessage;
  inserted: boolean;
  run_id?: string;
}

function isPostMessageResponse(value: unknown): value is PostMessageResponse {
  return isRecord(value) && isSessionMessage(value.message);
}

function isChartPreviewResponse(value: unknown): value is ChartPreviewResponse {
  return (
    isRecord(value) &&
    typeof value.preview_id === "string" &&
    isRecord(value.chart) &&
    isRecord(value.chart.option)
  );
}

export function getChartPreview(
  previewId: string,
  tokenOverride?: string,
): Promise<ChartPreviewResponse> {
  return runtimeFetch(
    `/api/chart-previews/${encodeURIComponent(previewId)}`,
    (raw) => decodeWith(raw, isChartPreviewResponse, "chart preview"),
    {},
    tokenOverride,
  );
}

export function listSessions(
  limit = 50,
  offset = 0,
): Promise<SessionSummary[]> {
  return runtimeFetch(`/api/sessions?limit=${limit}&offset=${offset}`, (raw) =>
    decodeWith(raw, isSessionSummaryList, "session list"),
  );
}

export function createSession(
  body: CreateSessionRequest,
): Promise<SessionSummary> {
  return runtimeFetch(
    "/api/sessions",
    (raw) => decodeWith(raw, isSessionSummary, "session"),
    {
      method: "POST",
      body: JSON.stringify(body),
    },
  );
}

export function getSession(sessionId: string): Promise<SessionDetail> {
  return runtimeFetch(`/api/sessions/${encodeURIComponent(sessionId)}`, (raw) =>
    decodeWith(raw, isSessionDetail, "session"),
  );
}

export function listSessionTraces(
  sessionId: string,
): Promise<RunTraceSummary[]> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/traces`,
    (raw) => decodeWith(raw, isRunTraceSummaryList, "session traces"),
  );
}

export function updateSession(
  sessionId: string,
  patch: { title?: string; status?: SessionStatus },
): Promise<SessionSummary> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}`,
    (raw) => decodeWith(raw, isSessionSummary, "session"),
    {
      method: "PATCH",
      body: JSON.stringify(patch),
    },
  );
}

export function deleteSession(sessionId: string): Promise<void> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}`,
    () => undefined,
    { method: "DELETE" },
  );
}

export function postMessage(
  sessionId: string,
  body: SendMessageRequest,
): Promise<PostMessageResponse> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/messages`,
    (raw) => decodeWith(raw, isPostMessageResponse, "message"),
    {
      method: "POST",
      body: JSON.stringify(body),
    },
  );
}

export function interruptSession(
  sessionId: string,
  runId?: string,
): Promise<SessionDetail> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/interrupt`,
    (raw) => decodeWith(raw, isSessionDetail, "session"),
    {
      method: "POST",
      body: JSON.stringify(runId ? { run_id: runId } : {}),
    },
  );
}

export function retrySession(sessionId: string): Promise<SessionDetail> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/retry`,
    (raw) => decodeWith(raw, isSessionDetail, "session"),
    {
      method: "POST",
      body: JSON.stringify({}),
    },
  );
}

export function compactSession(sessionId: string): Promise<SessionDetail> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/compact`,
    (raw) => decodeWith(raw, isSessionDetail, "session"),
    {
      method: "POST",
      body: JSON.stringify({}),
    },
  );
}

export function getToolCall(
  sessionId: string,
  toolCallId: string,
): Promise<ToolCallDetail> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/tool-calls/${encodeURIComponent(toolCallId)}`,
    (raw) => decodeWith(raw, isToolCallDetail, "tool call"),
  );
}

export function answerQuestion(
  sessionId: string,
  questionId: string,
  payload: { answers: QuestionAnswer[]; skipped: boolean },
): Promise<SessionDetail> {
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/questions/${encodeURIComponent(questionId)}/answer`,
    (raw) => decodeWith(raw, isSessionDetail, "session"),
    {
      method: "POST",
      body: JSON.stringify(payload),
    },
  );
}

export async function uploadAttachment(
  sessionId: string,
  file: File,
): Promise<AttachmentMeta> {
  const buffer = await file.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const chunkSize = 8192;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const slice = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode(...slice);
  }
  const content_base64 = window.btoa(binary);
  return runtimeFetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/attachments`,
    (raw) => decodeWith(raw, isAttachmentMeta, "attachment"),
    {
      method: "POST",
      body: JSON.stringify({
        filename: file.name,
        mime_type: file.type || "application/octet-stream",
        size_bytes: file.size,
        content_base64,
      }),
    },
  );
}

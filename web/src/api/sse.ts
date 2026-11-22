import { fetchEventSource } from "@microsoft/fetch-event-source";
import type {
  AgentPresenceEvent,
  CommentUpdatedEvent,
  EphemeralClearEvent,
  EphemeralPreviewEvent,
  RevisionDelta,
  SyncRequiredDelta,
} from "@/contracts/report-protocol";
import type { SessionEvent } from "@/contracts/session-protocol";
import { bearerHeaders, getAuthTokens } from "@/lib/auth";
import { isRecord, isStringArray } from "@/lib/guards";

function isSessionEvent(value: unknown): value is SessionEvent {
  return isRecord(value) && typeof value.event === "string";
}

function isRevisionDelta(value: unknown): value is RevisionDelta {
  return isRecord(value) && typeof value.revision === "number";
}

function isEphemeralPreviewEvent(
  value: unknown,
): value is EphemeralPreviewEvent {
  return isRecord(value) && typeof value.preview_id === "string";
}

function isEphemeralClearEvent(value: unknown): value is EphemeralClearEvent {
  return isRecord(value) && isStringArray(value.preview_ids);
}

function isAgentPresenceEvent(value: unknown): value is AgentPresenceEvent {
  return isRecord(value) && typeof value.status === "string";
}

function isCommentUpdatedEvent(value: unknown): value is CommentUpdatedEvent {
  return isRecord(value) && typeof value.thread_id === "string";
}

function isSyncRequiredDelta(value: unknown): value is SyncRequiredDelta {
  return isRecord(value) && typeof value.reason === "string";
}

export interface SessionStreamHandlers {
  onEvent: (event: SessionEvent) => void;
  onError?: (error: Error) => void;
}

const BASE_RECONNECT_DELAY_MS = 800;
const MAX_RECONNECT_DELAY_MS = 5000;

function waitForReconnect(attempt: number): Promise<void> {
  const backoff = Math.min(
    MAX_RECONNECT_DELAY_MS,
    BASE_RECONNECT_DELAY_MS * Math.pow(1.5, Math.min(attempt, 5)),
  );
  return new Promise((resolve) => {
    globalThis.setTimeout(resolve, Math.round(backoff));
  });
}

export function subscribeSessionEvents(
  sessionId: string,
  getAfterSeq: () => number,
  handlers: SessionStreamHandlers,
): () => void {
  const controller = new AbortController();
  let closed = false;
  let attempt = 0;
  const { runtimeToken } = getAuthTokens();

  const connect = async () => {
    for (;;) {
      if (closed) {
        return;
      }
      const afterSeq = getAfterSeq();
      try {
        await fetchEventSource(
          `/api/sessions/${encodeURIComponent(sessionId)}/events?afterSeq=${afterSeq}`,
          {
            signal: controller.signal,
            headers: {
              ...bearerHeaders(runtimeToken),
            },
            openWhenHidden: true,
            async onopen(response) {
              if (!response.ok) {
                throw new Error(
                  `Session SSE connection failed with status ${response.status}`,
                );
              }
              attempt = 0;
            },
            onmessage(msg) {
              if (!msg.data) {
                return;
              }
              try {
                const parsed: unknown = JSON.parse(msg.data);
                if (isSessionEvent(parsed)) {
                  handlers.onEvent(parsed);
                }
              } catch {
                // Ignore malformed keepalive/comment frames.
              }
            },
            onerror(err) {
              // Throw so the SDK does not retry the original afterSeq URL;
              // this loop reconnects with the freshest cursor instead.
              throw err;
            },
          },
        );
      } catch (err) {
        if (closed || controller.signal.aborted) {
          return;
        }
        attempt += 1;
        const error =
          err instanceof Error ? err : new Error("Session SSE error");
        handlers.onError?.(error);
      }
      if (closed) {
        return;
      }
      await waitForReconnect(attempt);
    }
  };

  void connect();

  return () => {
    closed = true;
    controller.abort();
  };
}

export interface DocumentStreamHandlers {
  onRevision: (delta: RevisionDelta) => void;
  onEphemeralPreview: (preview: EphemeralPreviewEvent) => void;
  onEphemeralClear: (clear: EphemeralClearEvent) => void;
  onAgentPresence: (presence: AgentPresenceEvent) => void;
  onCommentUpdated: (event: CommentUpdatedEvent) => void;
  onSyncRequired: (sync: SyncRequiredDelta) => void;
  onError?: (error: Error) => void;
}

export function subscribeDocumentEvents(
  docId: string,
  sinceRevOrGetter: number | (() => number),
  handlers: DocumentStreamHandlers,
): () => void {
  const controller = new AbortController();
  let closed = false;
  let attempt = 0;
  const { quireRoToken, quireRwToken } = getAuthTokens();
  const token = quireRoToken || quireRwToken;
  const getSinceRev =
    typeof sinceRevOrGetter === "function"
      ? sinceRevOrGetter
      : () => sinceRevOrGetter;

  const connect = async () => {
    for (;;) {
      if (closed) {
        return;
      }
      const sinceRev = getSinceRev();
      try {
        await fetchEventSource(
          `/api/docs/${encodeURIComponent(docId)}/events?sinceRev=${sinceRev}`,
          {
            signal: controller.signal,
            headers: {
              ...bearerHeaders(token),
            },
            openWhenHidden: true,
            async onopen(response) {
              if (!response.ok) {
                throw new Error(
                  `Document SSE connection failed with status ${response.status}`,
                );
              }
              attempt = 0;
            },
            onmessage(msg) {
              if (!msg.data) {
                return;
              }
              try {
                const data: unknown = JSON.parse(msg.data);
                switch (msg.event) {
                  case "revision":
                    if (isRevisionDelta(data)) {
                      handlers.onRevision(data);
                    }
                    break;
                  case "ephemeral_preview":
                    if (isEphemeralPreviewEvent(data)) {
                      handlers.onEphemeralPreview(data);
                    }
                    break;
                  case "ephemeral_clear":
                    if (isEphemeralClearEvent(data)) {
                      handlers.onEphemeralClear(data);
                    }
                    break;
                  case "agent_presence":
                    if (isAgentPresenceEvent(data)) {
                      handlers.onAgentPresence(data);
                    }
                    break;
                  case "comment_updated":
                    if (isCommentUpdatedEvent(data)) {
                      handlers.onCommentUpdated(data);
                    }
                    break;
                  case "sync_required":
                    if (isSyncRequiredDelta(data)) {
                      handlers.onSyncRequired(data);
                    }
                    break;
                  default:
                    break;
                }
              } catch {
                // Ignore malformed event payloads.
              }
            },
            onerror(err) {
              // Throw so fetchEventSource does not reconnect with a stale sinceRev URL;
              // our outer loop reconnects with the latest getSinceRev() watermark.
              throw err;
            },
          },
        );
      } catch (err) {
        if (closed || controller.signal.aborted) {
          return;
        }
        attempt += 1;
        const error =
          err instanceof Error ? err : new Error("Document SSE error");
        handlers.onError?.(error);
      }
      if (closed) {
        return;
      }
      await waitForReconnect(attempt);
    }
  };

  void connect();

  return () => {
    closed = true;
    controller.abort();
  };
}

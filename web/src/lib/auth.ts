import { isRecord } from "@/lib/guards";

export interface AuthTokens {
  runtimeToken: string;
  quireRoToken: string;
  quireRwToken: string;
}

const TOKEN_STORAGE_KEY = "quire.dev.tokens.v1";

function nonEmptyString(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

export function getAuthTokens(): AuthTokens {
  let stored: Partial<AuthTokens> = {};
  try {
    const raw = window.localStorage.getItem(TOKEN_STORAGE_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (isRecord(parsed)) {
        stored = {
          runtimeToken: nonEmptyString(parsed.runtimeToken),
          quireRoToken: nonEmptyString(parsed.quireRoToken),
          quireRwToken: nonEmptyString(parsed.quireRwToken),
        };
      }
    }
  } catch {
    // Ignore invalid storage entries.
  }
  const env = import.meta.env as Record<string, string | undefined>;
  const runtimeToken =
    stored.runtimeToken ?? nonEmptyString(env.VITE_RUNTIME_API_TOKEN) ?? "";
  const quireRwToken =
    stored.quireRwToken ??
    nonEmptyString(env.VITE_QUIRE_RW_TOKEN) ??
    runtimeToken;
  const quireRoToken =
    stored.quireRoToken ??
    nonEmptyString(env.VITE_QUIRE_RO_TOKEN) ??
    quireRwToken;
  return {
    runtimeToken,
    quireRoToken,
    quireRwToken,
  };
}

export function bearerHeaders(token: string): Record<string, string> {
  if (!token.trim()) {
    return {};
  }
  return { Authorization: `Bearer ${token.trim()}` };
}

import { isRecord } from "@/lib/guards";

export interface AuthTokens {
  runtimeToken: string;
  quireRoToken: string;
  quireRwToken: string;
}

const TOKEN_STORAGE_KEY = "quire.dev.tokens.v1";

export function getAuthTokens(): AuthTokens {
  let stored: Partial<AuthTokens> = {};
  try {
    const raw = window.localStorage.getItem(TOKEN_STORAGE_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (isRecord(parsed)) {
        stored = {
          runtimeToken:
            typeof parsed.runtimeToken === "string"
              ? parsed.runtimeToken
              : undefined,
          quireRoToken:
            typeof parsed.quireRoToken === "string"
              ? parsed.quireRoToken
              : undefined,
          quireRwToken:
            typeof parsed.quireRwToken === "string"
              ? parsed.quireRwToken
              : undefined,
        };
      }
    }
  } catch {
    // Ignore invalid storage entries.
  }
  const env = import.meta.env as Record<string, string | undefined>;
  return {
    runtimeToken: stored.runtimeToken ?? env.VITE_RUNTIME_API_TOKEN ?? "",
    quireRoToken:
      stored.quireRoToken ??
      env.VITE_QUIRE_RO_TOKEN ??
      env.VITE_QUIRE_RW_TOKEN ??
      "",
    quireRwToken: stored.quireRwToken ?? env.VITE_QUIRE_RW_TOKEN ?? "",
  };
}

export function saveAuthTokens(tokens: AuthTokens): void {
  try {
    window.localStorage.setItem(TOKEN_STORAGE_KEY, JSON.stringify(tokens));
  } catch {
    // Ignore storage errors.
  }
}

export function bearerHeaders(token: string): Record<string, string> {
  if (!token.trim()) {
    return {};
  }
  return { Authorization: `Bearer ${token.trim()}` };
}

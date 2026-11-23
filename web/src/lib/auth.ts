import { isRecord } from "@/lib/guards";

export type UserRole = "admin" | "user";

export interface AuthUser {
  user_id: string;
  username: string;
  email?: string | null;
  display_name: string;
  role: UserRole;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface AuthSession {
  token: string;
  user: AuthUser;
}

const AUTH_STORAGE_KEY = "quire.auth.session.v1";
const listeners = new Set<() => void>();

function notifyAuthListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

export function subscribeAuthSession(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function isAuthUser(value: unknown): value is AuthUser {
  return (
    isRecord(value) &&
    typeof value.user_id === "string" &&
    typeof value.username === "string" &&
    typeof value.display_name === "string" &&
    (value.role === "admin" || value.role === "user")
  );
}

export function getAuthSession(): AuthSession | null {
  try {
    const raw = window.localStorage.getItem(AUTH_STORAGE_KEY);
    if (!raw) {
      return null;
    }
    const parsed: unknown = JSON.parse(raw);
    if (
      isRecord(parsed) &&
      typeof parsed.token === "string" &&
      parsed.token.trim().length > 0 &&
      isAuthUser(parsed.user)
    ) {
      return {
        token: parsed.token.trim(),
        user: parsed.user,
      };
    }
  } catch {
    // Ignore malformed storage entries.
  }
  return null;
}

export function getAccessToken(): string {
  return getAuthSession()?.token ?? "";
}

export function setAuthSession(session: AuthSession): void {
  try {
    window.localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(session));
  } catch {
    // Ignore storage quota failures.
  }
  notifyAuthListeners();
}

export function updateStoredUser(user: AuthUser): void {
  const current = getAuthSession();
  if (!current) {
    return;
  }
  setAuthSession({ token: current.token, user });
}

export function clearAuthSession(): void {
  try {
    window.localStorage.removeItem(AUTH_STORAGE_KEY);
  } catch {
    // Ignore storage errors.
  }
  notifyAuthListeners();
}

export function bearerHeaders(token: string): Record<string, string> {
  if (!token.trim()) {
    return {};
  }
  return { Authorization: `Bearer ${token.trim()}` };
}

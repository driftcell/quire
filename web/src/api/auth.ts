import { ApiError } from "@/api/runtime";
import {
  bearerHeaders,
  clearAuthSession,
  getAccessToken,
  isAuthUser,
  setAuthSession,
  updateStoredUser,
  type AuthSession,
  type AuthUser,
} from "@/lib/auth";
import { isRecord } from "@/lib/guards";

async function parseError(response: Response): Promise<ApiError> {
  let code = `HTTP_${response.status}`;
  let message = response.statusText || "请求失败";
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
  return new ApiError(response.status, code, message, hint);
}

function decodeAuthSession(raw: unknown): AuthSession {
  if (
    !isRecord(raw) ||
    typeof raw.token !== "string" ||
    !raw.token.trim() ||
    !isAuthUser(raw.user)
  ) {
    throw new Error("Invalid auth response");
  }
  return { token: raw.token.trim(), user: raw.user };
}

export async function login(
  identifier: string,
  password: string,
): Promise<AuthSession> {
  const response = await fetch("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ identifier: identifier.trim(), password }),
  });
  if (!response.ok) {
    throw await parseError(response);
  }
  const session = decodeAuthSession(await response.json());
  setAuthSession(session);
  return session;
}

export async function registerAccount(payload: {
  username: string;
  email?: string;
  display_name?: string;
  password: string;
}): Promise<AuthSession> {
  const response = await fetch("/api/auth/register", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      username: payload.username.trim(),
      email: payload.email?.trim() || undefined,
      display_name: payload.display_name?.trim() || undefined,
      password: payload.password,
    }),
  });
  if (!response.ok) {
    throw await parseError(response);
  }
  const session = decodeAuthSession(await response.json());
  setAuthSession(session);
  return session;
}

export async function fetchMe(): Promise<AuthUser> {
  const token = getAccessToken();
  const response = await fetch("/api/auth/me", {
    headers: {
      "Content-Type": "application/json",
      ...bearerHeaders(token),
    },
  });
  if (!response.ok) {
    if (response.status === 401) {
      clearAuthSession();
    }
    throw await parseError(response);
  }
  const raw: unknown = await response.json();
  if (!isAuthUser(raw)) {
    throw new Error("Invalid user profile response");
  }
  updateStoredUser(raw);
  return raw;
}

export async function changePassword(
  currentPassword: string,
  newPassword: string,
): Promise<void> {
  const token = getAccessToken();
  const response = await fetch("/api/auth/change-password", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...bearerHeaders(token),
    },
    body: JSON.stringify({
      current_password: currentPassword,
      new_password: newPassword,
    }),
  });
  if (!response.ok) {
    throw await parseError(response);
  }
}

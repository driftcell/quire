import { ApiError } from "@/api/runtime";
import {
  bearerHeaders,
  getAccessToken,
  isAuthUser,
  type AuthUser,
  type UserRole,
} from "@/lib/auth";
import { isRecord } from "@/lib/guards";

export interface ModelProviderRecord {
  provider_id: string;
  name: string;
  protocol: "anthropic" | "openai_compatible";
  base_url: string;
  api_key_masked: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface ModelRecord {
  id: string;
  provider_id: string;
  model_id: string;
  display_name: string;
  context_limit: number;
  max_output_tokens?: number | null;
  supports_reasoning: boolean;
  supports_tools: boolean;
  supports_vision: boolean;
  enabled: boolean;
  updated_at: string;
}

export interface McpServerRecord {
  server_id: string;
  name: string;
  url: string;
  enabled: boolean;
  last_synced_at?: string | null;
  last_error?: string | null;
}

function decodeProvider(raw: unknown): ModelProviderRecord | null {
  if (!isRecord(raw)) return null;
  if (
    typeof raw.provider_id !== "string" ||
    typeof raw.name !== "string" ||
    (raw.protocol !== "anthropic" && raw.protocol !== "openai_compatible") ||
    typeof raw.base_url !== "string" ||
    typeof raw.api_key_masked !== "string" ||
    typeof raw.enabled !== "boolean" ||
    typeof raw.created_at !== "string" ||
    typeof raw.updated_at !== "string"
  ) {
    return null;
  }
  return {
    provider_id: raw.provider_id,
    name: raw.name,
    protocol: raw.protocol,
    base_url: raw.base_url,
    api_key_masked: raw.api_key_masked,
    enabled: raw.enabled,
    created_at: raw.created_at,
    updated_at: raw.updated_at,
  };
}

function decodeModel(raw: unknown): ModelRecord | null {
  if (!isRecord(raw)) return null;
  if (
    typeof raw.id !== "string" ||
    typeof raw.provider_id !== "string" ||
    typeof raw.model_id !== "string" ||
    typeof raw.display_name !== "string" ||
    typeof raw.context_limit !== "number" ||
    typeof raw.supports_reasoning !== "boolean" ||
    typeof raw.supports_tools !== "boolean" ||
    typeof raw.supports_vision !== "boolean" ||
    typeof raw.enabled !== "boolean" ||
    typeof raw.updated_at !== "string"
  ) {
    return null;
  }
  return {
    id: raw.id,
    provider_id: raw.provider_id,
    model_id: raw.model_id,
    display_name: raw.display_name,
    context_limit: raw.context_limit,
    max_output_tokens:
      typeof raw.max_output_tokens === "number" ? raw.max_output_tokens : null,
    supports_reasoning: raw.supports_reasoning,
    supports_tools: raw.supports_tools,
    supports_vision: raw.supports_vision,
    enabled: raw.enabled,
    updated_at: raw.updated_at,
  };
}

function decodeMcpServer(raw: unknown): McpServerRecord | null {
  if (!isRecord(raw)) return null;
  if (
    typeof raw.server_id !== "string" ||
    typeof raw.name !== "string" ||
    typeof raw.url !== "string" ||
    typeof raw.enabled !== "boolean"
  ) {
    return null;
  }
  return {
    server_id: raw.server_id,
    name: raw.name,
    url: raw.url,
    enabled: raw.enabled,
    last_synced_at:
      typeof raw.last_synced_at === "string" ? raw.last_synced_at : null,
    last_error: typeof raw.last_error === "string" ? raw.last_error : null,
  };
}

async function adminRequest(
  path: string,
  init: RequestInit = {},
): Promise<unknown> {
  const token = getAccessToken();
  const headers = new Headers(init.headers);
  if (!headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  for (const [k, v] of Object.entries(bearerHeaders(token))) {
    headers.set(k, v);
  }
  const response = await fetch(path, {
    ...init,
    headers,
  });
  if (!response.ok) {
    let code = `HTTP_${response.status}`;
    let message = response.statusText || "请求失败";
    let hint: string | undefined;
    try {
      const body: unknown = await response.json();
      if (isRecord(body)) {
        const err = isRecord(body.error) ? body.error : body;
        if (typeof err.code === "string") code = err.code;
        if (typeof err.message === "string") message = err.message;
        if (typeof err.hint === "string") hint = err.hint;
      }
    } catch {
      // Non-JSON response
    }
    throw new ApiError(response.status, code, message, hint);
  }
  const body: unknown = await response.json();
  return body;
}

export async function listAdminUsers(): Promise<AuthUser[]> {
  const data = await adminRequest("/api/admin/users");
  if (!Array.isArray(data)) return [];
  return data.filter((item): item is AuthUser => isAuthUser(item));
}

export async function updateAdminUser(
  userId: string,
  patch: { role?: UserRole; enabled?: boolean },
): Promise<AuthUser> {
  const data = await adminRequest(
    `/api/admin/users/${encodeURIComponent(userId)}`,
    {
      method: "PATCH",
      body: JSON.stringify(patch),
    },
  );
  if (!isAuthUser(data)) {
    throw new Error("Invalid user response");
  }
  return data;
}

export async function listModelProviders(): Promise<ModelProviderRecord[]> {
  const data = await adminRequest("/api/models/providers");
  if (!Array.isArray(data)) return [];
  return data
    .map(decodeProvider)
    .filter((item): item is ModelProviderRecord => item !== null);
}

export async function createModelProvider(input: {
  provider_id: string;
  name: string;
  protocol: "anthropic" | "openai_compatible";
  base_url: string;
  api_key_ref: string;
  enabled?: boolean;
}): Promise<ModelProviderRecord> {
  const data = await adminRequest("/api/models/providers", {
    method: "POST",
    body: JSON.stringify({
      ...input,
      enabled: input.enabled ?? true,
    }),
  });
  const decoded = decodeProvider(data);
  if (!decoded) {
    throw new Error("Invalid provider response");
  }
  return decoded;
}

export async function listModels(): Promise<ModelRecord[]> {
  const data = await adminRequest("/api/models");
  if (!Array.isArray(data)) return [];
  return data
    .map(decodeModel)
    .filter((item): item is ModelRecord => item !== null);
}

export async function createModel(input: {
  id: string;
  provider_id: string;
  model_id: string;
  display_name: string;
  context_limit: number;
  max_output_tokens?: number | null;
  supports_reasoning?: boolean;
  supports_tools?: boolean;
  supports_vision?: boolean;
  enabled?: boolean;
}): Promise<ModelRecord> {
  const data = await adminRequest("/api/models", {
    method: "POST",
    body: JSON.stringify({
      supports_reasoning: true,
      supports_tools: true,
      supports_vision: false,
      enabled: true,
      ...input,
    }),
  });
  const decoded = decodeModel(data);
  if (!decoded) {
    throw new Error("Invalid model response");
  }
  return decoded;
}

export async function updateModel(
  modelId: string,
  patch: { enabled?: boolean },
): Promise<ModelRecord> {
  const data = await adminRequest(
    `/api/models/${encodeURIComponent(modelId)}`,
    {
      method: "PATCH",
      body: JSON.stringify(patch),
    },
  );
  const decoded = decodeModel(data);
  if (!decoded) {
    throw new Error("Invalid model response");
  }
  return decoded;
}

export async function listMcpServers(): Promise<McpServerRecord[]> {
  const data = await adminRequest("/api/mcp/servers");
  if (!Array.isArray(data)) return [];
  return data
    .map(decodeMcpServer)
    .filter((item): item is McpServerRecord => item !== null);
}

export function syncMcpServer(serverId: string): Promise<unknown> {
  return adminRequest(`/api/mcp/servers/${encodeURIComponent(serverId)}/sync`, {
    method: "POST",
  });
}

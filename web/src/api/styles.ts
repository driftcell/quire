import { bearerHeaders, clearAuthSession, getAccessToken } from "@/lib/auth";
import { isRecord } from "@/lib/guards";
import {
  BUILTIN_STYLE_PRESETS,
  type StylePresetBundle,
  type StylePresetRecord,
} from "@/lib/stylePresets";
import { ApiError } from "@/api/runtime";

const LOCAL_STYLES_KEY = "quire.custom.styles.v1";

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

async function stylesFetch<T>(
  path: string,
  decode: (raw: unknown) => T,
  init: RequestInit = {},
): Promise<T> {
  const accessToken = getAccessToken();
  const response = await fetch(path, {
    ...init,
    headers: buildHeaders(accessToken, init.headers),
  });
  if (!response.ok) {
    if (response.status === 401) {
      clearAuthSession();
    }
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
      // Non-JSON error body
    }
    throw new ApiError(response.status, code, message, hint);
  }
  if (response.status === 204) {
    return decode(undefined);
  }
  const raw: unknown = await response.json();
  return decode(raw);
}

function isStylePresetRecord(value: unknown): value is StylePresetRecord {
  return (
    isRecord(value) &&
    typeof value.style_id === "string" &&
    typeof value.name === "string" &&
    typeof value.is_builtin === "boolean" &&
    typeof value.is_public === "boolean" &&
    isRecord(value.bundle) &&
    isRecord(value.bundle.theme)
  );
}

function isStylePresetList(value: unknown): value is StylePresetRecord[] {
  return Array.isArray(value) && value.every(isStylePresetRecord);
}

export function readLocalCustomStyles(): StylePresetRecord[] {
  if (typeof window === "undefined") {
    return [];
  }
  try {
    const raw = window.localStorage.getItem(LOCAL_STYLES_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    return isStylePresetList(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function writeLocalCustomStyles(styles: StylePresetRecord[]): void {
  if (typeof window === "undefined") {
    return;
  }
  try {
    window.localStorage.setItem(LOCAL_STYLES_KEY, JSON.stringify(styles));
  } catch {
    // Ignore storage quota errors
  }
}

export async function listStylePresets(): Promise<StylePresetRecord[]> {
  try {
    return await stylesFetch("/api/styles", (raw) => {
      if (!isStylePresetList(raw)) {
        throw new Error("Malformed style presets list");
      }
      return raw;
    });
  } catch {
    const custom = readLocalCustomStyles();
    return [...BUILTIN_STYLE_PRESETS, ...custom];
  }
}

export interface CreateStylePresetPayload {
  name: string;
  description?: string;
  category_tag?: string;
  is_public: boolean;
  base_preset_id: string;
  bundle: StylePresetBundle;
}

export async function createStylePreset(
  payload: CreateStylePresetPayload,
  fallbackOwner?: { userId?: string; displayName?: string },
): Promise<StylePresetRecord> {
  try {
    return await stylesFetch(
      "/api/styles",
      (raw) => {
        if (!isStylePresetRecord(raw)) {
          throw new Error("Malformed style preset response");
        }
        return raw;
      },
      {
        method: "POST",
        body: JSON.stringify(payload),
      },
    );
  } catch (err) {
    if (
      err instanceof ApiError &&
      err.status >= 400 &&
      err.status < 500 &&
      err.status !== 404
    ) {
      throw err;
    }
    const now = new Date().toISOString();
    const record: StylePresetRecord = {
      style_id: `stl_${Math.random().toString(36).slice(2, 10)}`,
      name: payload.name.trim(),
      description: (payload.description ?? "").trim(),
      category_tag: (payload.category_tag ?? "自定义").trim() || "自定义",
      is_builtin: false,
      is_public: payload.is_public,
      owner_user_id: fallbackOwner?.userId ?? "local",
      owner_display_name: fallbackOwner?.displayName ?? "当前用户",
      base_preset_id: payload.base_preset_id || "quire-folio",
      bundle: payload.bundle,
      created_at: now,
      updated_at: now,
    };
    const current = readLocalCustomStyles();
    writeLocalCustomStyles([record, ...current]);
    return record;
  }
}

export interface UpdateStylePresetPayload {
  name?: string;
  description?: string;
  category_tag?: string;
  is_public?: boolean;
  bundle?: StylePresetBundle;
}

export async function updateStylePreset(
  styleId: string,
  payload: UpdateStylePresetPayload,
): Promise<StylePresetRecord> {
  try {
    return await stylesFetch(
      `/api/styles/${encodeURIComponent(styleId)}`,
      (raw) => {
        if (!isStylePresetRecord(raw)) {
          throw new Error("Malformed style preset response");
        }
        return raw;
      },
      {
        method: "PATCH",
        body: JSON.stringify(payload),
      },
    );
  } catch (err) {
    if (
      err instanceof ApiError &&
      err.status >= 400 &&
      err.status < 500 &&
      err.status !== 404
    ) {
      throw err;
    }
    const current = readLocalCustomStyles();
    const idx = current.findIndex((item) => item.style_id === styleId);
    const existing = idx >= 0 ? current[idx] : undefined;
    if (!existing) {
      throw err;
    }
    const updated: StylePresetRecord = {
      ...existing,
      name: payload.name !== undefined ? payload.name.trim() : existing.name,
      description:
        payload.description !== undefined
          ? payload.description.trim()
          : existing.description,
      category_tag:
        payload.category_tag !== undefined
          ? payload.category_tag.trim()
          : existing.category_tag,
      is_public:
        payload.is_public !== undefined
          ? payload.is_public
          : existing.is_public,
      bundle: payload.bundle !== undefined ? payload.bundle : existing.bundle,
      updated_at: new Date().toISOString(),
    };
    const next = [...current];
    next[idx] = updated;
    writeLocalCustomStyles(next);
    return updated;
  }
}

export async function deleteStylePreset(styleId: string): Promise<void> {
  try {
    await stylesFetch(
      `/api/styles/${encodeURIComponent(styleId)}`,
      () => undefined,
      { method: "DELETE" },
    );
  } finally {
    const current = readLocalCustomStyles();
    if (current.some((item) => item.style_id === styleId)) {
      writeLocalCustomStyles(
        current.filter((item) => item.style_id !== styleId),
      );
    }
  }
}

export interface ApplyStyleToDocResponse {
  doc_id: string;
  revision: number;
  preset: string;
}

export async function applyStyleToDocument(
  docId: string,
  payload: {
    style_id?: string;
    bundle?: StylePresetBundle;
    sync_layout_variants?: boolean;
  },
): Promise<ApplyStyleToDocResponse> {
  return stylesFetch(
    `/api/docs/${encodeURIComponent(docId)}/apply-style`,
    (raw) => {
      if (
        !isRecord(raw) ||
        typeof raw.doc_id !== "string" ||
        typeof raw.revision !== "number"
      ) {
        throw new Error("Malformed apply-style response");
      }
      return {
        doc_id: raw.doc_id,
        revision: raw.revision,
        preset: typeof raw.preset === "string" ? raw.preset : "default",
      };
    },
    {
      method: "POST",
      body: JSON.stringify(payload),
    },
  );
}

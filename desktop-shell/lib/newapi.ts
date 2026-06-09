"use client";

export type NewApiUser = {
  id: string;
  username?: string | null;
  email?: string | null;
  phone?: string | null;
  avatar?: string | null;
  status?: string | null;
  role?: string | null;
  createdAt?: string | null;
  updatedAt?: string | null;
  raw: Record<string, unknown>;
};

export type NewApiSession = {
  accessToken: string | null;
  refreshToken: string | null;
  user: NewApiUser | null;
  raw: Record<string, unknown>;
};

export class NewApiError extends Error {
  status: number;
  payload: unknown;

  constructor(message: string, status = 500, payload: unknown = null) {
    super(message);
    this.name = "NewApiError";
    this.status = status;
    this.payload = payload;
  }
}

const SESSION_STORAGE_KEY = "newapi.session";

const DEFAULT_REGISTER_PATH = "/api/user/register";
const DEFAULT_LOGIN_PATH = "/api/user/login";
const DEFAULT_USER_DETAIL_PATH = "/api/user/:id";

type RegisterInput = {
  username: string;
  email: string;
  password: string;
};

type LoginInput = {
  account: string;
  password: string;
};

function envValue(name: string, fallback: string): string {
  const value = process.env[name];
  if (typeof value === "string" && value.trim()) {
    return value.trim();
  }
  return fallback;
}

function baseUrl(): string {
  return envValue("NEXT_PUBLIC_NEWAPI_BASE_URL", "");
}

function endpointUrl(path: string): string {
  const normalizedPath = path.startsWith("/") ? path : `/${path}`;
  const root = baseUrl();
  if (!root) {
    return normalizedPath;
  }
  return `${root.replace(/\/$/, "")}${normalizedPath}`;
}

function joinMessage(payload: unknown, fallback: string): string {
  if (!payload || typeof payload !== "object") {
    return fallback;
  }
  const source = payload as Record<string, unknown>;
  const parts = [source.message, source.msg, source.error, source.detail].filter(
    (item): item is string => typeof item === "string" && item.trim().length > 0,
  );
  return parts[0] || fallback;
}

function pickNestedValue(source: Record<string, unknown>, paths: string[][]): unknown {
  for (const path of paths) {
    let current: unknown = source;
    let matched = true;
    for (const segment of path) {
      if (!current || typeof current !== "object" || !(segment in current)) {
        matched = false;
        break;
      }
      current = (current as Record<string, unknown>)[segment];
    }
    if (matched && current !== undefined && current !== null) {
      return current;
    }
  }
  return null;
}

function normalizeUser(candidate: unknown, fallbackId?: string | null): NewApiUser | null {
  if (!candidate || typeof candidate !== "object") {
    if (!fallbackId) {
      return null;
    }
    return {
      id: fallbackId,
      raw: {},
    };
  }

  const raw = candidate as Record<string, unknown>;
  const rawId = raw.id ?? raw.userId ?? raw.uid ?? fallbackId ?? null;
  if (rawId == null) {
    return null;
  }

  return {
    id: String(rawId),
    username: typeof raw.username === "string" ? raw.username : typeof raw.name === "string" ? raw.name : null,
    email: typeof raw.email === "string" ? raw.email : null,
    phone: typeof raw.phone === "string" ? raw.phone : null,
    avatar:
      typeof raw.avatar === "string"
        ? raw.avatar
        : typeof raw.avatarUrl === "string"
          ? raw.avatarUrl
          : null,
    status: typeof raw.status === "string" ? raw.status : null,
    role: typeof raw.role === "string" ? raw.role : null,
    createdAt:
      typeof raw.createdAt === "string"
        ? raw.createdAt
        : typeof raw.created_at === "string"
          ? raw.created_at
          : null,
    updatedAt:
      typeof raw.updatedAt === "string"
        ? raw.updatedAt
        : typeof raw.updated_at === "string"
          ? raw.updated_at
          : null,
    raw,
  };
}

function normalizeSession(payload: unknown): NewApiSession {
  const raw = payload && typeof payload === "object" ? (payload as Record<string, unknown>) : {};
  const accessToken = pickNestedValue(raw, [
    ["accessToken"],
    ["access_token"],
    ["token"],
    ["data", "accessToken"],
    ["data", "access_token"],
    ["data", "token"],
  ]);
  const refreshToken = pickNestedValue(raw, [
    ["refreshToken"],
    ["refresh_token"],
    ["data", "refreshToken"],
    ["data", "refresh_token"],
  ]);
  const rawUser = pickNestedValue(raw, [
    ["user"],
    ["data", "user"],
    ["profile"],
    ["data", "profile"],
    ["data"],
  ]);
  const fallbackUserId = pickNestedValue(raw, [
    ["userId"],
    ["uid"],
    ["data", "userId"],
    ["data", "uid"],
  ]);

  return {
    accessToken: typeof accessToken === "string" ? accessToken : null,
    refreshToken: typeof refreshToken === "string" ? refreshToken : null,
    user: normalizeUser(rawUser, typeof fallbackUserId === "string" ? fallbackUserId : null),
    raw,
  };
}

async function requestJson<TPayload>(
  path: string,
  init: RequestInit,
  fallbackMessage: string,
): Promise<TPayload> {
  const response = await fetch(endpointUrl(path), {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init.headers || {}),
    },
    cache: "no-store",
  });

  const text = await response.text();
  const payload = text ? safeParseJson(text) : null;

  if (!response.ok) {
    throw new NewApiError(joinMessage(payload, fallbackMessage), response.status, payload);
  }

  return payload as TPayload;
}

function safeParseJson(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return { raw: value };
  }
}

export async function registerUser(input: RegisterInput): Promise<NewApiSession> {
  const payload = await requestJson<unknown>(
    envValue("NEXT_PUBLIC_NEWAPI_REGISTER_PATH", DEFAULT_REGISTER_PATH),
    {
      method: "POST",
      body: JSON.stringify({
        username: input.username.trim(),
        email: input.email.trim(),
        password: input.password,
      }),
    },
    "注册失败",
  );
  return normalizeSession(payload);
}

export async function loginUser(input: LoginInput): Promise<NewApiSession> {
  const payload = await requestJson<unknown>(
    envValue("NEXT_PUBLIC_NEWAPI_LOGIN_PATH", DEFAULT_LOGIN_PATH),
    {
      method: "POST",
      body: JSON.stringify({
        account: input.account.trim(),
        password: input.password,
      }),
    },
    "登录失败",
  );
  return normalizeSession(payload);
}

export async function fetchUserDetail(userId: string, accessToken?: string | null): Promise<NewApiUser> {
  const pathTemplate = envValue("NEXT_PUBLIC_NEWAPI_USER_DETAIL_PATH", DEFAULT_USER_DETAIL_PATH);
  const payload = await requestJson<unknown>(
    pathTemplate.replace(":id", encodeURIComponent(userId)),
    {
      method: "GET",
      headers: accessToken
        ? {
            Authorization: `Bearer ${accessToken}`,
          }
        : undefined,
    },
    "获取用户详情失败",
  );
  const normalized = normalizeUser(
    payload && typeof payload === "object" && "data" in (payload as Record<string, unknown>)
      ? (payload as Record<string, unknown>).data
      : payload,
    userId,
  );
  if (!normalized) {
    throw new NewApiError("接口返回了空的用户数据", 500, payload);
  }
  return normalized;
}

export function saveSession(session: NewApiSession) {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.setItem(SESSION_STORAGE_KEY, JSON.stringify(session));
}

export function readSession(): NewApiSession | null {
  if (typeof window === "undefined") {
    return null;
  }
  const raw = window.localStorage.getItem(SESSION_STORAGE_KEY);
  if (!raw) {
    return null;
  }
  try {
    return normalizeSession(JSON.parse(raw));
  } catch {
    return null;
  }
}

export function clearSession() {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.removeItem(SESSION_STORAGE_KEY);
}

// Typed client for the controller API (types generated from crates/server).

import type {
  ActivityKind,
  ActivityResponse,
  HistoryResponse,
  PauseResponse,
  Settings,
  SettingsPatch,
  StateResponse,
  ZonePatch,
} from '@aircon/api';

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    const detail =
      data && typeof data === 'object' && 'detail' in data ? String(data.detail) : res.statusText;
    throw new ApiError(res.status, detail || `HTTP ${res.status}`);
  }
  return data as T;
}

export const api = {
  state: () => request<StateResponse>('GET', '/api/state'),
  /** Run a check now and return the fresh state. */
  tick: () => request<StateResponse>('POST', '/api/tick'),
  history: (hours: number, step?: number) =>
    request<HistoryResponse>(
      'GET',
      `/api/history?hours=${hours}${step != null ? `&step=${step}` : ''}`,
    ),
  activity: (opts: { kind?: ActivityKind; before?: number; limit?: number } = {}) => {
    const q = new URLSearchParams();
    if (opts.kind) q.set('kind', opts.kind);
    if (opts.before != null) q.set('before', String(opts.before));
    if (opts.limit != null) q.set('limit', String(opts.limit));
    const qs = q.toString();
    return request<ActivityResponse>('GET', `/api/activity${qs ? `?${qs}` : ''}`);
  },
  settings: () => request<Settings>('GET', '/api/settings'),
  patchSettings: (patch: SettingsPatch) => request<Settings>('PATCH', '/api/settings', patch),
  patchZone: (zone: number, patch: ZonePatch) =>
    request<unknown>('PATCH', `/api/zones/${zone}`, patch),
  setAc: (on: boolean) => request<unknown>('PUT', '/api/ac', { on }),
  pause: (minutes: number) => request<PauseResponse>('POST', '/api/pause', { minutes }),
  resume: () => request<unknown>('POST', '/api/resume'),
};

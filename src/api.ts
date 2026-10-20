import { invoke } from "@tauri-apps/api/core";
export type Profile = {
  id: string;
  name: string;
  server: string;
  port: number;
  country: string;
  selected: boolean;
  protocol: string;
};
export type Event = {
  time: string;
  level: string;
  code: string;
  message: string;
};
export type Status = {
  version: string;
  state: string;
  message: string;
  busy: boolean;
  mode: string;
  profile_id: string;
  engine_running: boolean;
  engine_available: boolean;
  legacy_active: boolean;
  is_admin: boolean;
  uptime_seconds: number;
  exit_ip: string | null;
  latency_ms: number | null;
  last_error: { code: string; message: string } | null;
  download_bytes: number;
  upload_bytes: number;
  download_rate: number;
  upload_rate: number;
  history: { down: number; up: number }[];
  diagnostics: {
    host: string;
    status: string;
    latency_ms?: number;
    error?: string;
  }[];
  api_url: string;
  token_path: string;
  data_dir: string;
};
export const native =
  typeof window !== "undefined" && Boolean("__TAURI_INTERNALS__" in window);
export const initial: Status = {
  version: "0.4.0-preview.1",
  state: "disconnected",
  message: "Готов к подключению",
  busy: false,
  mode: "system",
  profile_id: "",
  engine_running: false,
  engine_available: false,
  legacy_active: false,
  is_admin: false,
  uptime_seconds: 0,
  exit_ip: null,
  latency_ms: null,
  last_error: null,
  download_bytes: 0,
  upload_bytes: 0,
  download_rate: 0,
  upload_rate: 0,
  history: [],
  diagnostics: [],
  api_url: "http://127.0.0.1:17871",
  token_path: "%LOCALAPPDATA%\\CorepassNext\\api-token",
  data_dir: "%LOCALAPPDATA%\\CorepassNext",
};
export async function api<T>(
  method: string,
  path: string,
  body: object = {},
): Promise<T> {
  if (!native) {
    if (method === "GET") {
      if (path === "/v1/status") return initial as T;
      if (path === "/v1/profiles") return { profiles: [] } as T;
      if (path === "/v1/logs") return { events: [] } as T;
      if (path === "/v1/settings")
        return { minimize_to_tray: true, mode: "system" } as T;
      if (path === "/v1/servers")
        return {
          servers: [],
          supported: true,
          busy: false,
          feature: "preview",
        } as T;
    }
    throw new Error(
      "Это предпросмотр интерфейса. Откройте настольное приложение.",
    );
  }
  return invoke<T>("api_request", { method, path, body });
}
export function message(e: unknown) {
  return e instanceof Error
    ? e.message
    : typeof e === "object" && e !== null && "message" in e
      ? String(e.message)
      : "Не удалось выполнить действие";
}
export function bytes(n: number) {
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${Math.round(n)} Б`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} КБ`;
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} МБ`;
  return `${(n / 1073741824).toFixed(2)} ГБ`;
}
export function duration(n: number) {
  return `${Math.floor(n / 3600)
    .toString()
    .padStart(2, "0")}:${Math.floor((n / 60) % 60)
    .toString()
    .padStart(2, "0")}:${Math.floor(n % 60)
    .toString()
    .padStart(2, "0")}`;
}

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppInfo, Environment, PortDiagnosis, ProjectGroup, Settings, Snapshot, StopReport } from "./types";

export function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Error text from a Rust command; commands return plain strings as errors. */
export class ApiError extends Error {}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop()) throw new ApiError("This view needs the Port Pilot desktop app.");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new ApiError(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  snapshot: () => call<Snapshot | null>("snapshot_get"),
  refresh: () => call<void>("refresh_now"),
  setPaused: (paused: boolean) => call<void>("set_paused", { paused }),
  settings: () => call<Settings>("settings_get"),
  saveSettings: (settings: Settings) => call<Settings>("settings_set", { settings }),
  setUiLanguage: (language: string) => call<void>("set_ui_language", { language }),
  stop: (listenerId: string, force: boolean, tree: boolean) => call<StopReport>("stop_listener", { listenerId, force, tree }),
  diagnose: (port: number) => call<PortDiagnosis>("diagnose_port", { port }),
  findFree: (range: string, count: number) => call<number[]>("find_free", { range, count }),
  environment: () => call<Environment>("environment"),
  projects: () => call<ProjectGroup[]>("projects_overview"),
  openUrl: (listenerId: string) => call<void>("open_url", { listenerId }),
  openPath: (path: string, how: "folder" | "terminal" | "editor") => call<void>("open_path", { path, how }),
};

export function onSnapshot(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("snapshot", (e) => cb(e.payload));
}

export function onPaused(cb: (paused: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("paused", (e) => cb(e.payload));
}

export function onFocusListener(cb: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>("focus-listener", (e) => cb(e.payload));
}

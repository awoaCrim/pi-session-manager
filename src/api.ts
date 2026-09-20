import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  BatchRequest,
  Bootstrap,
  SessionDetail,
  SessionIndex,
  TerminalPreference,
} from "../shared/types";

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri())
    throw new Error(
      "请通过 Pi Sessions 桌面应用打开。此界面不提供浏览器 WebUI 服务",
    );
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw error instanceof Error ? error : new Error(String(error));
  }
}
export const api = {
  bootstrap: () => call<Bootstrap>("bootstrap"),
  index: () => call<SessionIndex>("list_sessions"),
  refresh: () => call<SessionIndex>("refresh_sessions"),
  async subscribe(onChange: () => void, onError: (error: string) => void) {
    if (!isTauri()) return () => {};
    const stopChange = await listen("sessions-changed", onChange);
    try {
      const stopError = await listen<string>("sync-error", (event) =>
        onError(event.payload),
      );
      return () => {
        stopChange();
        stopError();
      };
    } catch (error) {
      stopChange();
      throw error;
    }
  },
  async detail(
    key: string,
    branch: string,
    limit: number,
    signal?: AbortSignal,
  ) {
    const data = await call<SessionDetail>("session_detail", {
      key,
      branch,
      limit,
    });
    if (signal?.aborted) throw new DOMException("Aborted", "AbortError");
    return data;
  },
  batch: (request: BatchRequest) => call<void>("batch_sessions", { request }),
  launch: (data: {
    key?: string;
    projectKey?: string;
    cwd?: string;
    fork?: boolean;
  }) =>
    call<{ message: string }>("launch_session", {
      key: data.key ?? null,
      projectKey: data.projectKey ?? null,
      cwd: data.cwd ?? null,
      fork: data.fork ?? false,
    }),
  export: (keys: string[]) =>
    call<{ saved: boolean; path?: string }>("export_sessions", { keys }),
  async chooseDirectory(defaultPath?: string) {
    const result = await open({
      directory: true,
      multiple: false,
      title: "选择目录",
      defaultPath,
    });
    return typeof result === "string" ? result : null;
  },
  setRoot: (root: string | null) =>
    call<Bootstrap>("set_session_root", { root }),
  setTerminal: (preference: TerminalPreference) =>
    call<Bootstrap>("set_terminal", { preference }),
  openExternal: async (url: string) => {
    if (/^https?:\/\//i.test(url)) await openUrl(url);
  },
};
export const formatNumber = (value: number) =>
  Intl.NumberFormat("en", {
    notation: value >= 10000 ? "compact" : "standard",
    maximumFractionDigits: 1,
  }).format(value);
export const formatDate = (value: string) =>
  value ? new Date(value).toLocaleString("zh-CN", { hour12: false }) : "未知";
export const relativeTime = (value: string) => {
  const minutes = Math.max(0, (Date.now() - new Date(value).getTime()) / 60000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${Math.floor(minutes)} 分钟前`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时前`;
  if (minutes < 10080) return `${Math.floor(minutes / 1440)} 天前`;
  return new Date(value).toLocaleDateString("zh-CN", {
    month: "short",
    day: "numeric",
  });
};

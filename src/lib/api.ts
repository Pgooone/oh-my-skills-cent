import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "./runtime";

/**
 * 唯一知道「invoke 还是 fetch」的地方。
 * Tauri 运行时 → tauri invoke；浏览器 → POST /api/commands/{command}（契约见设计 §2.3）。
 */

let realBackend = isTauriRuntime();
let readOnly = false;
let probePromise: Promise<boolean> | null = null;

/** 是否存在真实后端（Tauri，或 Web 且 /api/health 探测通过）。同步读取最近一次探测结果。 */
export function hasRealBackend(): boolean {
  return realBackend;
}

/** 是否只读模式（Tauri/无后端恒 false；仅 Web 壳经 /api/health 的 readonly 字段探测）。同步读取最近一次探测结果。 */
export function isReadonly(): boolean {
  return readOnly;
}

/** 探测 Web 后端；Tauri 下恒为 true（readonly 恒 false）。多次调用共享同一次探测。 */
export function probeRealBackend(): Promise<boolean> {
  if (isTauriRuntime()) {
    realBackend = true;
    readOnly = false;
    return Promise.resolve(true);
  }
  if (!probePromise) {
    probePromise = fetch("/api/health")
      .then((response) => (response.ok ? response.json() : null))
      .catch(() => null)
      .then((body) => {
        realBackend = body !== null;
        const health = body as { readonly?: unknown } | null;
        readOnly = health !== null && typeof health.readonly === "boolean" && health.readonly;
        return realBackend;
      });
  }
  return probePromise;
}

export async function callApi<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauriRuntime()) {
    return invoke<T>(command, args);
  }
  const response = await fetch(`/api/commands/${command}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args ?? {})
  });
  if (!response.ok) {
    let message = `请求失败（HTTP ${response.status}）`;
    try {
      const body: unknown = await response.json();
      if (body && typeof (body as { error?: unknown }).error === "string") {
        message = (body as { error: string }).error;
      }
    } catch {
      // 非 JSON 错误体时保留默认信息
    }
    throw new Error(message);
  }
  return (await response.json()) as T;
}

// 模块加载即开始探测，尽量赶在首次渲染前完成。
void probeRealBackend();

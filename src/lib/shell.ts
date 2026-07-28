import { confirm, open } from "@tauri-apps/plugin-dialog";
import { callApi } from "./api";
import { isTauriRuntime } from "./runtime";

/**
 * 桌面能力替代的统一入口：Tauri 走插件/命令，Web 走就地降级。
 * UI 组件不感知运行时，全部经这里调用。
 */

export async function pickDirectory(title: string): Promise<string | null> {
  if (isTauriRuntime()) {
    const selected = await open({ directory: true, multiple: false, title });
    return typeof selected === "string" ? selected : null;
  }
  // TODO(dir-browser): 批次 3 替换为 DirPicker modal（promise 化挂载点），
  // 当前临时用 window.prompt 手动输入路径。
  const entered = window.prompt(`${title}：请输入目录路径`);
  const trimmed = entered?.trim() ?? "";
  return trimmed ? trimmed : null;
}

export function openUrl(url: string): void {
  if (isTauriRuntime()) {
    void callApi("open_url", { url });
    return;
  }
  window.open(url, "_blank");
}

export function revealPath(path: string): void {
  if (isTauriRuntime()) {
    void callApi("open_path", { path });
    return;
  }
  // Web 降级：浏览器无法打开本机路径，以可复制的方式展示。
  window.prompt("浏览器中无法直接打开路径，请手动复制：", path);
}

export async function askConfirm(message: string, title: string): Promise<boolean> {
  if (isTauriRuntime()) {
    return confirm(message, { title, kind: "warning" });
  }
  return window.confirm(message);
}

import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { DirPicker } from "../components/DirPicker";
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
  return pickDirectoryWeb(title);
}

/** Web 分支：挂载 DirPicker modal，选择/取消时 resolve 并卸载。 */
function pickDirectoryWeb(title: string): Promise<string | null> {
  return new Promise((resolve) => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    let settled = false;
    const finish = (path: string | null) => {
      if (settled) return;
      settled = true;
      resolve(path);
      // 延迟卸载，避免在 React 事件回调中同步 unmount 根节点。
      setTimeout(() => {
        root.unmount();
        host.remove();
      }, 0);
    };
    root.render(
      createElement(DirPicker, {
        title,
        onSelect: (path: string) => finish(path),
        onCancel: () => finish(null)
      })
    );
  });
}

export function openUrl(url: string): void {
  if (isTauriRuntime()) {
    void callApi("open_url", { url });
    return;
  }
  window.open(url, "_blank");
}

/**
 * 保存导出包：Tauri 走 plugin-dialog save 选路径后经 save_export_to_path 写文件，
 * Web 走 Blob 下载。返回是否实际保存（取消选择 = false）。
 */
export async function saveExportPackage(filename: string, base64: string): Promise<boolean> {
  if (isTauriRuntime()) {
    const path = await save({
      defaultPath: filename,
      filters: [{ name: "工作流分享包", extensions: ["zip"] }]
    });
    if (typeof path !== "string") return false;
    await callApi("save_export_to_path", { path, base64 });
    return true;
  }
  downloadBlob(base64, filename);
  return true;
}

/** Web 分支：base64 → Blob 触发浏览器下载。 */
function downloadBlob(base64: string, filename: string): void {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/zip" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
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

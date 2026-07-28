import { confirm, open } from "@tauri-apps/plugin-dialog";
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

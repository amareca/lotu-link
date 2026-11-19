import { isTauri } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

/**
 * System clipboard access.
 *
 * In the Tauri app the plugin reads/writes the OS clipboard natively, so
 * WebView2 never shows a clipboard permission prompt. The browser fallback
 * exists only for `pnpm dev` without the native shell.
 */
export async function readSystemClipboard(): Promise<string> {
  if (isTauri()) return readText();
  return navigator.clipboard.readText();
}

export async function writeSystemClipboard(text: string): Promise<void> {
  if (isTauri()) {
    await writeText(text);
    return;
  }
  await navigator.clipboard.writeText(text);
}

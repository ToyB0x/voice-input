import { invoke, isTauri } from "@tauri-apps/api/core";
type Status = { configured: boolean; working: boolean; installing: boolean; currentVersion: string; availableVersion: string | null; phase: string; message: string; downloaded: number; total: number | null };
let installing = false;
export const updateInstalling = () => installing;
export function setupUpdates() {
  const info = document.getElementById("update-info")!;
  const error = document.getElementById("update-error")!;
  const check = document.getElementById("update-check") as HTMLButtonElement;
  const apply = document.getElementById("update-install") as HTMLButtonElement;
  let reading = false;
  let pending = false;
  let latest: Status | null = null;
  const setText = (element: HTMLElement, text: string) => {
    // Preserve native WebKit hit targets while status polling is unchanged.
    if (element.textContent !== text) element.textContent = text;
  };
  function render() {
    if (!latest) return;
    const s = latest;
    installing = s.installing;
    setText(info, `v${s.currentVersion}${s.availableVersion ? ` → v${s.availableVersion}` : ""}: ${s.message}${s.installing ? ` (${s.downloaded} / ${s.total ?? "?"} bytes)` : ""}`);
    check.disabled = pending || !s.configured || s.working;
    apply.disabled = pending || s.working || !s.availableVersion;
  }
  async function refresh() {
    if (reading || pending || !isTauri()) return;
    reading = true;
    try { latest = await invoke<Status>("update_status"); render(); }
    catch (cause) { setText(error, String(cause)); }
    finally { reading = false; }
  }
  for (const [button, command] of [[check, "check_updates"], [apply, "install_update"]] as const) {
    button.addEventListener("click", () => void (async () => {
      if (pending) return;
      pending = true;
      check.disabled = true;
      apply.disabled = true;
      setText(error, command === "install_update" ? "更新の開始を確認中…" : "更新を確認中…");
      try { latest = await invoke<Status>(command); setText(error, ""); }
      catch (cause) { setText(error, String(cause)); }
      finally { pending = false; render(); }
      await refresh();
    })());
  }
  if (isTauri()) { void refresh(); window.setInterval(() => void refresh(), 1000); }
}

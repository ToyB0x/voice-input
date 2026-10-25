import { invoke, isTauri } from "@tauri-apps/api/core";
type Status = { configured: boolean; working: boolean; installing: boolean; currentVersion: string; availableVersion: string | null; phase: string; message: string; downloaded: number; total: number | null };
let installing=false;
export const updateInstalling=()=>installing;
export function setupUpdates() {
  const info=document.getElementById("update-info")!;
  const error=document.getElementById("update-error")!;
  const check=document.getElementById("update-check") as HTMLButtonElement;
  const apply=document.getElementById("update-install") as HTMLButtonElement;
  let reading=false;
  async function refresh() {
    if (reading || !isTauri()) return;
    reading=true;
    try { const s=await invoke<Status>("update_status"); installing=s.installing;
      info.textContent=`v${s.currentVersion}${s.availableVersion?` → v${s.availableVersion}`:""}: ${s.message}${s.installing?` (${s.downloaded} / ${s.total??"?"} bytes)`:""}`;
      check.disabled=!s.configured || s.working; apply.disabled=s.working || !s.availableVersion;
    } catch (cause) {error.textContent=String(cause);} finally {reading=false;}
  }
  for (const [button,command] of [[check,"check_updates"],[apply,"install_update"]] as const) button.addEventListener("click",()=>void (async()=>{
    try {await invoke(command);error.textContent="";}catch(cause){error.textContent=String(cause);}await refresh();
  })());
  if (isTauri()){void refresh();window.setInterval(()=>void refresh(),1000);}
}

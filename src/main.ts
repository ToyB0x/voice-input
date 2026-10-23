import "./styles.css";
import { invoke, isTauri } from "@tauri-apps/api/core";

type Permission = "not_determined" | "authorized" | "denied" | "restricted" | "unavailable";
type Snapshot = {
  phase: "idle" | "recording" | "recorded" | "error";
  permission: Permission;
  elapsedMs: number;
  level: number;
  sampleCount: number;
  sampleRate: number;
  error: string | null;
};

const start = document.querySelector<HTMLButtonElement>("#start")!;
const stop = document.querySelector<HTMLButtonElement>("#stop")!;
const clear = document.querySelector<HTMLButtonElement>("#clear")!;
const settings = document.querySelector<HTMLButtonElement>("#settings")!;
const permission = document.querySelector<HTMLParagraphElement>("#permission")!;
const status = document.querySelector<HTMLParagraphElement>("#status")!;
const error = document.querySelector<HTMLParagraphElement>("#error")!;
const level = document.querySelector<HTMLMeterElement>("#input-level")!;
const duration = document.querySelector<HTMLOutputElement>("#duration")!;

const permissions: Record<Permission, string> = {
  not_determined: "未確認（録音開始時に macOS が確認します）",
  authorized: "許可済み",
  denied: "拒否済み（システム設定で許可してください）",
  restricted: "制限されています（この Mac の管理設定を確認してください）",
  unavailable: "利用できません",
};

let snapshot: Snapshot | undefined;
let busy = false;
let refreshing = false;
let actionError: string | null = null;

function render() {
  if (!snapshot) return;
  permission.textContent = `マイクの許可: ${permissions[snapshot.permission]}`;
  const recording = snapshot.phase === "recording";
  start.disabled = busy || recording || ["denied", "restricted", "unavailable"].includes(snapshot.permission);
  stop.disabled = busy || !recording;
  clear.disabled = busy || recording || (snapshot.sampleCount === 0 && snapshot.phase !== "error");
  settings.hidden = snapshot.permission !== "denied";
  settings.disabled = busy;
  level.value = snapshot.level;
  duration.textContent = `${(snapshot.elapsedMs / 1000).toFixed(1)} 秒`;
  const states: Record<Snapshot["phase"], string> = {
    idle: "録音を開始できます。",
    recording: "録音中…",
    recorded: `録音を停止しました。${snapshot.sampleCount.toLocaleString()} サンプルを Mac 内に保持しています。`,
    error: "録音できませんでした。",
  };
  status.textContent = busy ? "処理中です。マイクの確認が表示されたら選択してください。" : states[snapshot.phase];
  const message = actionError ?? snapshot.error;
  error.hidden = !message;
  error.textContent = message;
}

async function refresh() {
  if (busy || refreshing) return;
  refreshing = true;
  try {
    const next = await invoke<Snapshot>("recording_status");
    if (!busy) {
      snapshot = next;
      render();
    }
  } catch (cause) {
    error.hidden = false;
    error.textContent = `録音の状態を取得できません: ${String(cause)}`;
  } finally {
    refreshing = false;
  }
}

async function action(operation: () => Promise<unknown>) {
  if (busy) return;
  busy = true;
  actionError = null;
  render();
  try {
    await operation();
  } catch (cause) {
    actionError = String(cause);
  } finally {
    busy = false;
    await refresh();
    render();
  }
}

start.addEventListener("click", () => void action(async () => {
  const allowed = await invoke<Permission>("request_microphone_permission");
  if (allowed !== "authorized") {
    throw new Error(`マイクの許可: ${permissions[allowed]}`);
  }
  await invoke("start_recording");
}));
stop.addEventListener("click", () => void action(() => invoke("stop_recording")));
clear.addEventListener("click", () => void action(() => invoke("clear_recording")));
settings.addEventListener("click", () => void action(() => invoke("open_microphone_settings")));

if (isTauri()) {
  void refresh();
  window.setInterval(() => void refresh(), 150);
} else {
  permission.textContent = "録音は macOS アプリで利用できます。";
  status.textContent = "ブラウザーでは UI のプレビューのみ利用できます。";
}

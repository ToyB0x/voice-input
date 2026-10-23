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
  modelReady: boolean;
  modelPath: string;
  backend: string;
  working: boolean;
  recognitionPhase: string;
  transcript: string | null;
  recognitionError: string | null;
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

const modelPath = document.querySelector<HTMLInputElement>("#model-path")!;
const backend = document.querySelector<HTMLSelectElement>("#backend")!;
const loadModel = document.querySelector<HTMLButtonElement>("#load-model")!;
const modelStatus = document.querySelector<HTMLParagraphElement>("#model-status")!;
const transcribe = document.querySelector<HTMLButtonElement>("#transcribe")!;
const transcript = document.querySelector<HTMLTextAreaElement>("#transcript")!;
const copy = document.querySelector<HTMLButtonElement>("#copy")!;
const copyStatus = document.querySelector<HTMLSpanElement>("#copy-status")!;

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
let refreshVersion = 0;
let actionError: string | null = null;

function setText(element: HTMLElement, text: string | null) {
  const value = text ?? "";
  if (element.textContent !== value) element.textContent = value;
}

function render() {
  if (!snapshot) return;
  setText(permission, `マイクの許可: ${permissions[snapshot.permission]}`);
  const recording = snapshot.phase === "recording";
  const working = snapshot.working;
  start.disabled = busy || working || recording || ["denied", "restricted", "unavailable"].includes(snapshot.permission);
  stop.disabled = busy || !recording;
  clear.disabled = busy || (snapshot.recognitionPhase === "discarding") ||
    (!working && snapshot.sampleCount === 0 && snapshot.phase !== "error" && snapshot.transcript === null && !snapshot.recognitionError);
  modelPath.disabled = backend.disabled = busy || working || recording;
  loadModel.disabled = busy || working || recording || !modelPath.value.trim();
  transcribe.disabled = busy || working || recording || !snapshot.modelReady || snapshot.sampleCount === 0;
  copy.disabled = busy || working || !snapshot.transcript;
  if (transcript.value !== (snapshot.transcript ?? "")) {
    transcript.value = snapshot.transcript ?? "";
    copyStatus.textContent = "";
  }
  const modelStates: Record<string, string> = {
    loading: "モデルを読み込み中…",
    transcribing: "文字起こし中…（この Mac 内で処理しています）",
    discarding: "結果を破棄しました。実行中の処理の終了を待っています…",
  };
  setText(modelStatus, modelStates[snapshot.recognitionPhase] ??
    (snapshot.modelReady ? `モデル準備完了（${snapshot.backend === "metal" ? "GPU / Metal" : "CPU"}を選択）` : "モデル未読み込み。録音のみ利用できます。"));
  settings.hidden = snapshot.permission !== "denied";
  settings.disabled = busy;
  level.value = snapshot.level;
  setText(duration, `${(snapshot.elapsedMs / 1000).toFixed(1)} 秒`);
  const states: Record<Snapshot["phase"], string> = {
    idle: snapshot.permission === "authorized"
      ? "録音を開始できます。"
      : snapshot.permission === "not_determined"
        ? "録音開始時にマイクへのアクセスを確認します。"
        : "録音にはマイクへのアクセス許可が必要です。",
    recording: "録音中…",
    recorded: `録音を停止しました。${snapshot.sampleCount.toLocaleString()} サンプルを Mac 内に保持しています。`,
    error: "録音できませんでした。",
  };
  const resultStatus = snapshot.transcript !== null
    ? (snapshot.transcript ? "文字起こしが完了しました。" : "音声が検出されませんでした。結果は空です。")
    : states[snapshot.phase];
  setText(status, working ? modelStatus.textContent : busy ? "処理中です。マイクの確認が表示されたら選択してください。" : resultStatus);
  const message = actionError ?? snapshot.error ?? snapshot.recognitionError;
  error.hidden = !message;
  setText(error, message);
}

async function refresh() {
  if (busy || refreshing) return;
  refreshing = true;
  const version = ++refreshVersion;
  try {
    const next = await invoke<Snapshot>("recording_status");
    if (!busy && version === refreshVersion) {
      snapshot = next;
      render();
    }
  } catch (cause) {
    if (version === refreshVersion) {
      error.hidden = false;
      error.textContent = `録音の状態を取得できません: ${String(cause)}`;
    }
  } finally {
    if (version === refreshVersion) refreshing = false;
  }
}

async function action(operation: () => Promise<unknown>) {
  if (busy) return;
  busy = true;
  ++refreshVersion;
  refreshing = false;
  actionError = null;
  render();
  try {
    await operation();
  } catch (cause) {
    actionError = cause instanceof Error ? cause.message : String(cause);
  } finally {
    busy = false;
    await refresh();
    render();
  }
}

modelPath.addEventListener("input", render);
loadModel.addEventListener("click", () => void action(() => invoke("load_model", { path: modelPath.value, backend: backend.value })));
transcribe.addEventListener("click", () => void action(() => invoke("transcribe_recording")));
copy.addEventListener("click", () => void action(async () => {
  await invoke("copy_transcript");
  copyStatus.textContent = "コピーしました。";
}));

start.addEventListener("click", () => void action(async () => {
  copyStatus.textContent = "";
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

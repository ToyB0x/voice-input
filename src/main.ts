import "./styles.css";
import { setupModels } from "./model-setup";
import { invoke, isTauri } from "@tauri-apps/api/core";

type Preferences = { version: number; modelPath: string; backend: string; shortcut: string; autoInsert: boolean; recognitionContext: boolean; ocrContext: boolean; whisperContext: boolean; correctionContext: boolean; correctionModelPath: string; correctionBackend: string };
type InsertionStatus = { enabled: boolean; trusted: boolean; state: string; message: string };
type DesktopStatus = { preferences: Preferences; shortcutRegistered: boolean; error: string | null };

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
  modelName: string;
  modelBytes: number;
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
const modelInfo = document.querySelector<HTMLParagraphElement>("#model-info")!;
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

const shortcut = document.querySelector<HTMLInputElement>("#shortcut")!;
const savePreferences = document.querySelector<HTMLButtonElement>("#save-preferences")!;
const desktopStatus = document.querySelector<HTMLParagraphElement>("#desktop-status")!;
const preferencesMessage = document.querySelector<HTMLParagraphElement>("#preferences-message")!;
const autoInsert = document.querySelector<HTMLInputElement>("#auto-insert")!;
const insertionStatus = document.querySelector<HTMLParagraphElement>("#insertion-status")!;
const insertionMessage = document.querySelector<HTMLParagraphElement>("#insertion-message")!;
const recognitionContext = document.querySelector<HTMLInputElement>("#recognition-context")!;
const contextStatus = document.querySelector<HTMLParagraphElement>("#context-status")!;
type ContextStatus = { enabled: boolean; message: string; ocrMessage: string; screenPermitted: boolean };
type CorrectionStatus = { working: boolean; text: string | null; error: string | null; model: string; elapsedMs: number };
let context: ContextStatus | undefined;
let correction: CorrectionStatus | undefined;
const ocrContext = document.querySelector<HTMLInputElement>("#ocr-context")!;
const whisperContext = document.querySelector<HTMLInputElement>("#whisper-context")!;
const correctionContext = document.querySelector<HTMLInputElement>("#correction-context")!;
const ocrStatus = document.querySelector<HTMLParagraphElement>("#ocr-status")!;
const screenPermission = document.querySelector<HTMLButtonElement>("#screen-permission")!;
const correctionModelPath = document.querySelector<HTMLInputElement>("#correction-model-path")!;
const correctionBackend = document.querySelector<HTMLSelectElement>("#correction-backend")!;
const correct = document.querySelector<HTMLButtonElement>("#correct")!;
const corrected = document.querySelector<HTMLTextAreaElement>("#corrected")!;
const correctionStatus = document.querySelector<HTMLParagraphElement>("#correction-status")!;
const copyCorrection = document.querySelector<HTMLButtonElement>("#copy-correction")!;
const accessibility = document.querySelector<HTMLButtonElement>("#accessibility")!;
let insertion: InsertionStatus | undefined;
let preferencesInitialized = false;
let desktop: DesktopStatus | undefined;
let snapshot: Snapshot | undefined;
let busy = false;
let refreshing = false;
let refreshVersion = 0;
let actionError: string | null = null;

const managedModels = setupModels(
  () => busy || !snapshot || snapshot.working || snapshot.phase === "recording" || !!correction?.working,
  (speechPath, finalModelPath) => {
    if (speechPath !== undefined) modelPath.value = speechPath;
    if (finalModelPath !== undefined) correctionModelPath.value = finalModelPath;
    preferencesMessage.textContent = speechPath !== undefined ? "モデルを設定欄へ反映しました。保存し、音声モデルを読み込んでください。" : "補正モデルの設定を反映しました。設定を保存してください。";
    render();
  },
);

function setText(element: HTMLElement, text: string | null) {
  const value = text ?? "";
  if (element.textContent !== value) element.textContent = value;
}

function render() {
  managedModels.refreshControls();
  if (!snapshot) return;
  setText(permission, `マイクの許可: ${permissions[snapshot.permission]}`);
  savePreferences.disabled = busy;
  shortcut.disabled = autoInsert.disabled = recognitionContext.disabled = accessibility.disabled = busy;
  setText(contextStatus, context?.message ?? "認識補助: 無効");
  ocrContext.disabled = whisperContext.disabled = correctionContext.disabled = screenPermission.disabled = busy;
  correctionModelPath.disabled = correctionBackend.disabled = busy;
  setText(ocrStatus, `${context?.ocrMessage ?? "OCR: 無効"} / 画面収録${context?.screenPermitted ? "許可済み" : "未許可"}`);
  correct.disabled = busy || snapshot.working || snapshot.phase === "recording" || !!correction?.working || !snapshot.transcript || !correctionModelPath.value.trim();
  copyCorrection.disabled = busy || !!correction?.working || !correction?.text;
  if (corrected.value !== (correction?.text ?? "")) corrected.value = correction?.text ?? "";
  setText(correctionStatus, correction?.working ? "ローカル補正中…（破棄・新しい録音で取り消せます）" : correction?.error ?? (correction?.text !== null && correction?.text !== undefined ? `補正プレビュー: ${correction.model} / ${(correction.elapsedMs / 1000).toFixed(1)}秒。内容を確認してください。` : "補正は未実行です。元の文字起こしを保持します。"));
  setText(insertionStatus, !insertion?.enabled ? "直接入力: 無効" :
    `直接入力: 有効 / アクセシビリティ${insertion.trusted ? "許可済み" : "未許可"}`);
  setText(insertionMessage, insertion?.message ?? "");
  setText(desktopStatus, desktop?.error ?? (desktop?.shortcutRegistered
    ? `録音ショートカット: ${desktop.preferences.shortcut}（登録済み）`
    : "録音ショートカット: 無効または未登録。画面・メニューバーから録音できます。"));
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
  modelInfo.hidden = !snapshot.modelReady;
  setText(modelInfo, snapshot.modelReady ? `モデル: ${snapshot.modelName} / ${(snapshot.modelBytes / 1_000_000).toFixed(0)} MB（ファイル容量）` : "");
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
    const [next, nextDesktop, nextInsertion, nextContext, nextCorrection] = await Promise.all([
      invoke<Snapshot>("recording_status"), invoke<DesktopStatus>("desktop_status"),
      invoke<InsertionStatus>("insertion_status"),
      invoke<ContextStatus>("context_status"),
      invoke<CorrectionStatus>("correction_status"),
    ]);
    if (!busy && version === refreshVersion) {
      snapshot = next;
      desktop = nextDesktop;
      insertion = nextInsertion;
      context = nextContext;
      correction = nextCorrection;
      if (!preferencesInitialized) {
        modelPath.value = desktop.preferences.modelPath;
        backend.value = desktop.preferences.backend;
        shortcut.value = desktop.preferences.shortcut;
        autoInsert.checked = desktop.preferences.autoInsert;
        recognitionContext.checked = desktop.preferences.recognitionContext;
        ocrContext.checked = desktop.preferences.ocrContext;
        whisperContext.checked = desktop.preferences.whisperContext;
        correctionContext.checked = desktop.preferences.correctionContext;
        correctionModelPath.value = desktop.preferences.correctionModelPath;
        correctionBackend.value = desktop.preferences.correctionBackend;
        preferencesInitialized = true;
      }
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

savePreferences.addEventListener("click", () => void action(async () => {
  desktop = await invoke<DesktopStatus>("save_preferences", { preferences: {
    version: 1, modelPath: modelPath.value, backend: backend.value, shortcut: shortcut.value, autoInsert: autoInsert.checked, recognitionContext: recognitionContext.checked, ocrContext: ocrContext.checked,
    whisperContext: whisperContext.checked, correctionContext: correctionContext.checked,
    correctionModelPath: correctionModelPath.value, correctionBackend: correctionBackend.value,
  } });
  preferencesMessage.textContent = "設定を保存しました。ショートカットは反映済みです。";
}));
accessibility.addEventListener("click", () => void action(() => invoke("request_accessibility")));
for (const field of [modelPath, backend, shortcut, autoInsert, recognitionContext, ocrContext, whisperContext, correctionContext, correctionModelPath, correctionBackend]) {
  field.addEventListener("input", () => { preferencesMessage.textContent = "未保存の設定があります。"; });
}
modelPath.addEventListener("input", render);
correctionModelPath.addEventListener("input", render);
screenPermission.addEventListener("click", () => void action(() => invoke("request_screen_permission")));
correct.addEventListener("click", () => void action(async () => { correction = await invoke<CorrectionStatus>("correct_transcript"); }));
copyCorrection.addEventListener("click", () => void action(() => invoke("copy_correction")));
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

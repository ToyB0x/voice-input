import { setupComparison, comparisonWorking } from "./comparison";
import { setupUpdates, updateInstalling } from "./updates";
import "./styles.css";
import { setupModels } from "./model-setup";
import { setupInputLevel } from "./input-level";
import { invoke, isTauri } from "@tauri-apps/api/core";

type Preferences = { version: number; modelPath: string; backend: string; shortcut: string; autoInsert: boolean; autoCorrection: boolean; reuseCorrectionModel: boolean; showOverlay: boolean; recognitionContext: boolean; ocrContext: boolean; whisperContext: boolean; correctionContext: boolean; correctionModelPath: string; correctionEngine: string; correctionBackend: string };
type InsertionStatus = { enabled: boolean; trusted: boolean; state: string; message: string };
type DesktopStatus = { preferences: Preferences; shortcutRegistered: boolean; error: string | null };

type Permission = "not_determined" | "authorized" | "denied" | "restricted" | "unavailable";
type Snapshot = {
  phase: "idle" | "recording" | "recorded" | "error";
  permission: Permission;
  elapsedMs: number;
  recognitionMs?: number | null;
  level: number;
  sampleCount: number;
  sampleRate: number;
  inputDevice: string | null;
  inputChannels: number;
  error: string | null;
  modelReady: boolean;
  modelPath: string;
  modelName: string;
  modelBytes: number;
  backend: string;
  working: boolean;
  recognitionPhase: string;
  transcript: string | null;
  finalText: string | null;
  finalSource: string;
  recognitionError: string | null;
};

const start = document.querySelector<HTMLButtonElement>("#start")!;
const stop = document.querySelector<HTMLButtonElement>("#stop")!;
const clear = document.querySelector<HTMLButtonElement>("#clear")!;
const settings = document.querySelector<HTMLButtonElement>("#settings")!;
const permission = document.querySelector<HTMLParagraphElement>("#permission")!;
const status = document.querySelector<HTMLParagraphElement>("#status")!;
const error = document.querySelector<HTMLParagraphElement>("#error")!;
const level = document.querySelector<HTMLDivElement>("#input-level")!;
const levelFill = document.querySelector<HTMLSpanElement>("#input-level-fill")!;
const levelValue = document.querySelector<HTMLOutputElement>("#input-level-value")!;
const levelStatus = document.querySelector<HTMLParagraphElement>("#input-level-status")!;
const inputDevice = document.querySelector<HTMLParagraphElement>("#input-device")!;
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
const showOverlay = document.querySelector<HTMLInputElement>("#show-overlay")!;
const reuseCorrectionModel = document.querySelector<HTMLInputElement>("#reuse-correction-model")!;
const processingTimings = document.querySelector<HTMLParagraphElement>("#processing-timings")!;
const autoCorrection = document.querySelector<HTMLInputElement>("#auto-correction")!;
const finalText = document.querySelector<HTMLTextAreaElement>("#final-text")!;
const finalStatus = document.querySelector<HTMLParagraphElement>("#final-status")!;
const copyFinal = document.querySelector<HTMLButtonElement>("#copy-final")!;
const autoInsert = document.querySelector<HTMLInputElement>("#auto-insert")!;
const insertionStatus = document.querySelector<HTMLParagraphElement>("#insertion-status")!;
const insertionMessage = document.querySelector<HTMLParagraphElement>("#insertion-message")!;
const recognitionContext = document.querySelector<HTMLInputElement>("#recognition-context")!;
const contextStatus = document.querySelector<HTMLParagraphElement>("#context-status")!;
type ContextStatus = { enabled: boolean; message: string; ocrMessage: string; screenPermitted: boolean };
type FoundationStatus = { availability: string; framework_imported: boolean; os_supported: boolean; os_major: number; os_minor: number; os_patch: number };
type Timings = { totalMs: number; modelReused: boolean; backendMs: number | null; modelLoadMs: number | null; contextMs: number | null; prefillMs: number | null; generationMs: number | null; validationMs: number | null };
type CorrectionStatus = { checkingFoundation: boolean; foundation: FoundationStatus | null; working: boolean; timings?: Timings | null; text: string | null; error: string | null; model: string; elapsedMs: number };
let context: ContextStatus | undefined;
let correction: CorrectionStatus | undefined;
const ocrContext = document.querySelector<HTMLInputElement>("#ocr-context")!;
const whisperContext = document.querySelector<HTMLInputElement>("#whisper-context")!;
const correctionContext = document.querySelector<HTMLInputElement>("#correction-context")!;
const ocrStatus = document.querySelector<HTMLParagraphElement>("#ocr-status")!;
const screenPermission = document.querySelector<HTMLButtonElement>("#screen-permission")!;
const correctionEngine = document.querySelector<HTMLSelectElement>("#correction-engine")!;
const foundationDiagnostics = document.querySelector<HTMLDetailsElement>("#foundation-diagnostics")!;
const foundationCheck = document.querySelector<HTMLButtonElement>("#foundation-check")!;
const foundationCopy = document.querySelector<HTMLButtonElement>("#foundation-copy")!;
const foundationInfo = document.querySelector<HTMLParagraphElement>("#foundation-info")!;
const foundationDetail = document.querySelector<HTMLPreElement>("#foundation-detail")!;
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
const inputLevel = setupInputLevel(level, levelFill, levelValue, levelStatus, () => isTauri() && !busy);

const managedModels = setupModels(
  () => busy || !snapshot || snapshot.working || snapshot.phase === "recording" || !!correction?.working,
  (speechPath, finalModelPath) => {
    if (speechPath !== undefined) modelPath.value = speechPath;
    if (finalModelPath !== undefined) { correctionModelPath.value = finalModelPath; correctionEngine.value = "qwen3"; }
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
  shortcut.disabled = showOverlay.disabled = autoCorrection.disabled = autoInsert.disabled = recognitionContext.disabled = accessibility.disabled = busy;
  setText(contextStatus, context?.message ?? "認識補助: 無効");
  ocrContext.disabled = whisperContext.disabled = correctionContext.disabled = screenPermission.disabled = busy;
  const foundationSelected = correctionEngine.value === "apple-foundation";
  correctionEngine.disabled = busy;
  correctionModelPath.disabled = correctionBackend.disabled = reuseCorrectionModel.disabled = busy || foundationSelected;
  foundationCheck.disabled = busy || !!correction?.working || snapshot.working || snapshot.phase === "recording";
  foundationCopy.disabled = busy;
  const availabilityMessages: Record<string, string> = {
    available: "利用できます（実際の補正の成功はプレビューで確認してください）",
    osTooOld: "macOS 26以降が必要です", frameworkMissingFromSDK: "Xcode 26以降でヘルパーを再ビルドしてください",
    deviceNotEligible: "Apple Intelligenceに対応していないMacです",
    appleIntelligenceNotEnabled: "システム設定でApple Intelligenceを有効にしてください",
    modelNotReady: "OSのモデル準備完了を待って再確認してください",
    unsupportedLanguageOrLocale: "日本語を利用できるOS・モデルの設定を確認してください",
  };
  const afm = correction?.foundation;
  setText(foundationInfo, afm ? `Appleモデル: ${availabilityMessages[afm.availability] ?? afm.availability}` : "Appleモデルの利用可否は未確認です。");
  setText(foundationDetail, JSON.stringify({ availability: afm ?? null, lastError: correction?.error ?? null, lastElapsedMs: correction?.elapsedMs ?? 0 }, null, 2));
  setText(ocrStatus, `${context?.ocrMessage ?? "OCR: 無効"} / 画面収録${context?.screenPermitted ? "許可済み" : "未許可"}`);
  correct.disabled = busy || snapshot.working || snapshot.phase === "recording" || !!correction?.working || !snapshot.transcript || (!foundationSelected && !correctionModelPath.value.trim());
  copyCorrection.disabled = busy || !!correction?.working || !correction?.text;
  if (corrected.value !== (correction?.text ?? "")) corrected.value = correction?.text ?? "";
  const seconds = (ms: number | null | undefined) => ms == null ? "未計測" : `${(ms / 1000).toFixed(2)}秒`;
  const timing = correction?.timings;
  setText(processingTimings, `音声認識: ${seconds(snapshot.recognitionMs)} / 補正全体: ${seconds(timing?.totalMs)}` +
    (timing ? ` / モデル: ${timing.modelLoadMs == null ? "未計測（AppleはOS管理）" : timing.modelReused ? "再利用" : "新規"} / 初期化: ${seconds(timing.backendMs)} / 読込: ${seconds(timing.modelLoadMs)} / 文脈準備: ${seconds(timing.contextMs)} / 入力処理: ${seconds(timing.prefillMs)} / 生成: ${seconds(timing.generationMs)} / 検証: ${seconds(timing.validationMs)}` : ""));
  setText(correctionStatus, correction?.working ? (correction.checkingFoundation ? "Appleモデルの利用可否を確認中…" : "ローカル補正中…（破棄・新しい録音で取り消せます）") : correction?.error ?? (correction?.text !== null && correction?.text !== undefined ? `補正プレビュー: ${correction.model} / ${(correction.elapsedMs / 1000).toFixed(1)}秒。内容を確認してください。` : "補正は未実行です。元の文字起こしを保持します。"));
  setText(insertionStatus, !insertion?.enabled ? "直接入力: 無効" :
    `直接入力: 有効 / アクセシビリティ${insertion.trusted ? "許可済み" : "未許可"}`);
  setText(insertionMessage, insertion?.message ?? "");
  setText(desktopStatus, desktop?.error ?? (desktop?.shortcutRegistered
    ? `録音ショートカット: ${desktop.preferences.shortcut}（登録済み）`
    : "録音ショートカット: 無効または未登録。画面・メニューバーから録音できます。"));
  const recording = snapshot.phase === "recording";
  const working = snapshot.working || comparisonWorking() || updateInstalling();
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
  if (finalText.value !== (snapshot.finalText ?? "")) finalText.value = snapshot.finalText ?? "";
  copyFinal.disabled = busy || snapshot.finalSource === "correcting" || !snapshot.finalText;
  const finalMessages: Record<string,string> = {
    whisper:"最終結果: Whisper原文", correcting:"最終結果: 自動補正中。直接入力は補正完了まで待機します。",
    correction:"最終結果: 補正を採用しました。", fallback:"最終結果: 補正が失敗・棄却されたためWhisper原文を使用しました。",
    cancelled:"最終結果: 自動補正を取り消しました。直接入力は行いません。",
  };
  setText(finalStatus, finalMessages[snapshot.finalSource] ?? "最終結果は未作成です。");
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
  inputLevel.setRecording(recording, snapshot.level);
  setText(inputDevice, snapshot.inputDevice
    ? `録音入力: ${snapshot.inputDevice} / ${snapshot.sampleRate.toLocaleString()} Hz / ${snapshot.inputChannels} ch`
    : "録音入力: 録音開始時にmacOSの既定入力を使用します。");
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
        autoCorrection.checked = desktop.preferences.autoCorrection;
        reuseCorrectionModel.checked = desktop.preferences.reuseCorrectionModel ?? false;
        showOverlay.checked = desktop.preferences.showOverlay;
        recognitionContext.checked = desktop.preferences.recognitionContext;
        ocrContext.checked = desktop.preferences.ocrContext;
        whisperContext.checked = desktop.preferences.whisperContext;
        correctionContext.checked = desktop.preferences.correctionContext;
        correctionEngine.value = desktop.preferences.correctionEngine;
        if (correctionEngine.value === "apple-foundation") foundationDiagnostics.open = true;
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
    version: 1, modelPath: modelPath.value, backend: backend.value, shortcut: shortcut.value, autoInsert: autoInsert.checked, autoCorrection: autoCorrection.checked, reuseCorrectionModel: reuseCorrectionModel.checked, showOverlay: showOverlay.checked, recognitionContext: recognitionContext.checked, ocrContext: ocrContext.checked,
    whisperContext: whisperContext.checked, correctionContext: correctionContext.checked,
    correctionModelPath: correctionModelPath.value, correctionEngine: correctionEngine.value, correctionBackend: correctionBackend.value,
  } });
  preferencesMessage.textContent = "設定を保存しました。ショートカットは反映済みです。";
}));
accessibility.addEventListener("click", () => void action(() => invoke("request_accessibility")));
for (const field of [modelPath, backend, shortcut, autoInsert, autoCorrection, reuseCorrectionModel, showOverlay, recognitionContext, ocrContext, whisperContext, correctionContext, correctionEngine, correctionModelPath, correctionBackend]) {
  field.addEventListener("input", () => { preferencesMessage.textContent = "未保存の設定があります。"; });
}
modelPath.addEventListener("input", render);
correctionModelPath.addEventListener("input", render);
correctionEngine.addEventListener("change", () => {
  if (correctionEngine.value === "apple-foundation") foundationDiagnostics.open = true;
  render();
});
foundationCheck.addEventListener("click", () => void action(async () => { correction = await invoke<CorrectionStatus>("check_foundation_model"); }));
foundationCopy.addEventListener("click", () => void action(async () => { await invoke("copy_foundation_diagnostics"); preferencesMessage.textContent = "本文を含まない診断情報をコピーしました。"; }));
screenPermission.addEventListener("click", () => void action(() => invoke("request_screen_permission")));
correct.addEventListener("click", () => void action(async () => { correction = await invoke<CorrectionStatus>("correct_transcript"); }));
copyFinal.addEventListener("click", () => void action(() => invoke("copy_final_transcript")));
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

setupComparison();
setupUpdates();

if (isTauri()) {
  void refresh();
  window.setInterval(() => void refresh(), 150);
} else {
  permission.textContent = "録音は macOS アプリで利用できます。";
  status.textContent = "ブラウザーでは UI のプレビューのみ利用できます。";
}

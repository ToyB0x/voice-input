import { invoke, isTauri } from "@tauri-apps/api/core";

type Model = { id: string; name: string; kind: string; bytes: number; license: string; memoryHint: string; cached: boolean };
type Prepared = { id: string; path: string };
type Preparation = { generation: number; phase: string; modelId: string; received: number; total: number; index: number; count: number; message: string; prepared: Prepared[] };
const presets: Record<string, [string, string, string]> = {
  light: ["base", "", "軽量：Whisper base。補正を使わず認識結果を利用します。"],
  accurate: ["medium", "", "認識重視：Whisper medium。今回の固定音声での候補です。"],
  turbo: ["large-v3-turbo", "", "大きい音声モデル：Whisper large-v3-turbo。実機の速度とメモリをご確認ください。"],
  format: ["medium", "qwen3-4b", "整形比較：Whisper medium + Qwen3 4B。補正は確認後にコピーします。画面情報の取得元・利用先は別途選択してください。"],
};
export function setupModels(locked: () => boolean, apply: (speech: string, correction: string) => void) {
  const select = <T extends HTMLElement>(id: string) => document.querySelector<T>(`#${id}`)!;
  const preset = select<HTMLSelectElement>("model-preset");
  const speech = select<HTMLSelectElement>("managed-speech");
  const correction = select<HTMLSelectElement>("managed-correction");
  const download = select<HTMLButtonElement>("download-models");
  const verify = select<HTMLButtonElement>("verify-models");
  const applyButton = select<HTMLButtonElement>("apply-models");
  const cancel = select<HTMLButtonElement>("cancel-models");
  const information = select<HTMLParagraphElement>("managed-info");
  const description = select<HTMLParagraphElement>("preset-info");
  const message = select<HTMLParagraphElement>("managed-status");
  const progress = select<HTMLProgressElement>("model-progress");
  let models: Model[] = [];
  let state: Preparation | undefined;
  let requesting = false;
  let polling = false;
  let ignoredGeneration = -1;
  let terminalGeneration = -1;
  const ids = () => [speech.value, correction.value].filter(Boolean);
  const busy = () => requesting || !!state && ["starting", "downloading", "verifying"].includes(state.phase);
  const matches = () => !!state && state.phase === "ready" && state.generation !== ignoredGeneration && state.prepared.map(m => m.id).join() === ids().join();
  function render() {
    const unavailable = !isTauri() || !models.length;
    preset.disabled = speech.disabled = correction.disabled = unavailable || busy() || locked();
    download.disabled = unavailable || busy() || locked();
    verify.disabled = unavailable || busy() || locked() || ids().some(id => !models.find(m => m.id === id)?.cached);
    applyButton.disabled = !matches() || busy() || locked();
    cancel.disabled = requesting || !busy();
    const chosen = ids().map(id => models.find(m => m.id === id)).filter((m): m is Model => !!m);
    const total = chosen.reduce((n, m) => n + m.bytes, 0);
    download.textContent = `ダウンロードして準備（最大 ${(total / 1e9).toFixed(2)} GB）`;
    information.textContent = chosen.map(m => `${m.name} / ${(m.bytes / 1e9).toFixed(2)} GB / ${m.license} / ${m.cached ? "保存あり・使用前に検証" : "未取得"}。${m.memoryHint}`).join("\n");
    progress.hidden = !busy();
    progress.max = Math.max(state?.total ?? 0, 1);
    progress.value = state?.received ?? 0;
  }
  function reset() { state = undefined; description.textContent = presets[preset.value]?.[2] ?? "準備するモデルを個別に選択できます。"; message.textContent = "準備・検証後に設定欄へ反映してください。"; render(); }
  preset.addEventListener("change", () => { const p = presets[preset.value]; if (p) { speech.value = p[0]; correction.value = p[1]; } reset(); });
  for (const field of [speech, correction]) field.addEventListener("change", () => { preset.value = "custom"; reset(); });
  async function prepare(allowDownload: boolean) {
    if (busy() || locked()) return;
    requesting = true; ignoredGeneration = -1; message.textContent = "準備を開始しています…"; render();
    try { state = await invoke<Preparation>("prepare_models", { ids: ids(), download: allowDownload }); }
    catch (cause) { message.textContent = String(cause); }
    finally { requesting = false; render(); }
  }
  download.addEventListener("click", () => void prepare(true));
  verify.addEventListener("click", () => void prepare(false));
  cancel.addEventListener("click", () => { ignoredGeneration = state?.generation ?? -1; void invoke("cancel_model_preparation").catch(cause => { message.textContent = String(cause); }); });
  applyButton.addEventListener("click", () => {
    if (!matches() || locked()) return;
    apply(state!.prepared.find(m => m.id === speech.value)!.path, state!.prepared.find(m => m.id === correction.value)?.path ?? "");
    message.textContent = "設定欄へ反映しました。「設定を保存」と音声モデルの「モデルを読み込む」で適用してください。";
  });
  async function loadCatalog() {
    models = await invoke<Model[]>("model_catalog");
    if (!speech.options.length) {
      for (const m of models) (m.kind === "transcription" ? speech : correction).add(new Option(m.name, m.id));
      speech.value = "base"; correction.value = "";
    }
    render();
  }
  async function poll() {
    if (polling || requesting) return;
    polling = true;
    try {
      const next = await invoke<Preparation>("model_preparation_status");
      if (state && next.generation === state.generation && state.generation !== ignoredGeneration) {
        const changed = state.phase !== next.phase || state.received !== next.received;
        state = next;
        if (changed) message.textContent = `${state.message}${busy() ? ` ${state.index}/${state.count} · ${(state.received / 1e6).toFixed(0)} / ${(state.total / 1e6).toFixed(0)} MB` : ""}`;
      } else if (state && next.generation === state.generation) {
        state = next; message.textContent = "準備の反映を取り消しました。検証済みモデルは保存先に残ります。";
      }
      if (state && !busy() && state.generation !== terminalGeneration) { terminalGeneration = state.generation; await loadCatalog(); }
      render();
    } catch (cause) { message.textContent = String(cause); }
    finally { polling = false; }
  }
  if (isTauri()) { void loadCatalog().catch(cause => { message.textContent = String(cause); }); window.setInterval(() => void poll(), 300); }
  render();
  return { refreshControls: render };
}

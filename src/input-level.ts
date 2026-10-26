import { invoke } from "@tauri-apps/api/core";

type Level = { phase: string; level: number };

export function setupInputLevel(meter: HTMLMeterElement, readout: HTMLOutputElement, status: HTMLElement, canRead: () => boolean) {
  let recording = false;
  let reading = false;
  let generation = 0;
  let timer: number | undefined;
  let normalLevel = 0;
  let lastFastUpdate: number | undefined;
  let lastError = "";

  function text(element: HTMLElement, value: string) {
    if (element.textContent !== value) element.textContent = value;
  }
  function display(raw: number, message: string) {
    const rms = Number.isFinite(raw) ? Math.max(0, Math.min(1, raw)) : 0;
    const db = rms > 0 ? 20 * Math.log10(rms) : -Infinity;
    // Speech is often far below full scale; use a standard -60..0 dBFS display
    // without changing the captured samples or recognition input.
    meter.value = Math.max(0, (Math.max(-60, db) + 60) / 60);
    const label = `${Number.isFinite(db) ? db.toFixed(1) : "-∞"} dBFS`;
    text(readout, label);
    meter.setAttribute("aria-valuetext", label);
    text(status, message);
  }
  function fallback() {
    display(normalLevel, lastError ? `通常更新で表示しています。高速更新エラー: ${lastError}` : "通常更新で表示しています。");
  }

  function setRecording(active: boolean, level = 0) {
    normalLevel = level;
    if (active !== recording) {
      recording = active;
      generation++;
      lastFastUpdate = undefined;
      lastError = "";
      if (active) {
        fallback();
        timer = window.setInterval(() => void poll(), 50);
        void poll();
      } else {
        window.clearInterval(timer);
        timer = undefined;
        meter.value = 0;
        meter.removeAttribute("aria-valuetext");
        text(readout, "停止中");
        text(status, "");
      }
    } else if (active && (lastFastUpdate === undefined || performance.now() - lastFastUpdate > 250)) {
      // Keep displaying the regular recorder snapshot if the fast IPC is denied,
      // delayed, or never returns. A display failure must not look like silence.
      fallback();
    }
  }

  async function poll() {
    if (!recording || reading || !canRead()) return;
    const requested = generation;
    reading = true;
    try {
      const next = await invoke<Level>("recording_level");
      if (!recording || requested !== generation || !canRead()) return;
      if (next.phase !== "recording") { setRecording(false); return; }
      lastFastUpdate = performance.now();
      lastError = "";
      display(next.level, "");
    } catch (cause) {
      if (recording && requested === generation) {
        lastFastUpdate = undefined;
        lastError = String(cause).slice(0, 240);
        fallback();
      }
    } finally { reading = false; }
  }

  return { setRecording };
}

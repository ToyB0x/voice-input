import { invoke } from "@tauri-apps/api/core";

type Level = { phase: string; level: number };

// Poll only during recording. Metadata and OS permission queries belong to the
// slower status refresh; the meter must not wait for those responses.
export function setupInputLevel(meter: HTMLMeterElement, canRead: () => boolean) {
  let recording = false;
  let reading = false;
  let generation = 0;
  let timer: number | undefined;

  function setRecording(active: boolean) {
    if (active === recording) return;
    recording = active;
    generation++;
    if (active) {
      timer = window.setInterval(() => void poll(), 50);
      void poll();
    } else {
      window.clearInterval(timer);
      timer = undefined;
      meter.value = 0;
      meter.title = "";
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
      meter.value = Number.isFinite(next.level) ? Math.max(0, Math.min(1, next.level)) : 0;
      meter.title = "";
    } catch {
      if (recording && requested === generation) {
        meter.value = 0;
        meter.title = "入力レベルを取得できません。";
      }
    } finally { reading = false; }
  }

  return { setRecording };
}

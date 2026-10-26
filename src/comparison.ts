import { invoke, isTauri } from "@tauri-apps/api/core";

type Choice = { id: string; name: string; kind: string; available: boolean };
type Score = { cer: number | null; edits: number; referenceChars: number; silenceFalsePositive: boolean | null };
type Row = { index: number; case: {speech: string; speechBackend: string; correction: string; correctionBackend: string; source: string; route: string}; status: string; message: string; raw: string | null; candidate: string | null; finalText: string | null; asrMs: number | null; asrLoadMs: number | null; correctionMs: number | null; totalMs: number; asrScore: Score | null; candidateScore: Score | null; finalScore: Score | null };
type FoundationDiagnostic = {status: {availability: string; os_major: number; os_minor: number; os_patch: number; framework_imported: boolean} | null; error: string | null; elapsedMs: number};
type Status = {generation: number; working: boolean; armed: boolean; phase: string; message: string; total: number; completed: number; audioMs: number; axCharacters: number; ocrCharacters: number; hasReference: boolean; foundation: FoundationDiagnostic | null; rows: Row[]; next: number; more: boolean};
const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
let state: Status | undefined;
let generation = 0;
let cursor = 0;
let reading = false;
let acting = false;
let choices: Choice[] = [];
export const comparisonWorking = () => !!state?.working;

function selected(id: string): string[] {
  return Array.from(el<HTMLSelectElement>(id).selectedOptions, o => o.value);
}
function selection() {
  return {speech: selected("compare-speech"), speechBackends: selected("compare-asr-backends"), corrections: selected("compare-correction"), correctionBackends: selected("compare-correction-backends"), sources: selected("compare-sources"), routes: selected("compare-routes"), reference: el<HTMLInputElement>("compare-use-reference").checked ? el<HTMLTextAreaElement>("compare-reference").value : null};
}
function message(text: string) { el("compare-message").textContent = text; }
function seconds(ms: number | null) { return ms === null ? "—" : `${(ms/1000).toFixed(2)}秒`; }
function metric(score: Score | null) {
  if (!score) return "基準文なし";
  if (score.cer === null) return score.silenceFalsePositive ? "無音基準: 誤出力あり" : "無音基準: 出力なし";
  return `${(score.cer*100).toFixed(1)}% (${score.edits}/${score.referenceChars})`;
}
function cell(row: HTMLTableRowElement, text: string) {
  const td = row.insertCell(); td.textContent = text; return td;
}
function append(result: Row) {
  const tr = el<HTMLTableSectionElement>("compare-rows").insertRow();
  cell(tr, `${result.index+1}: ${result.case.speech} / ${result.case.speechBackend}\n${result.case.correction} / ${result.case.correctionBackend}\n${result.case.source} → ${result.case.route}`);
  cell(tr, `${result.status}\n${result.message}${["error", "unavailable", "rejected"].includes(result.status) && result.raw !== null ? "\nWhisper原文を保持しています。" : ""}`);
  cell(tr, `ASR ${seconds(result.asrMs)} (load ${seconds(result.asrLoadMs)})\n補正 ${seconds(result.correctionMs)}\n合計 ${seconds(result.totalMs)}`);
  cell(tr, `ASR ${metric(result.asrScore)}\n候補 ${metric(result.candidateScore)}\n最終 ${metric(result.finalScore)}`);
  const td = cell(tr, "");
  const details = document.createElement("details");
  const summary = document.createElement("summary"); summary.textContent = "認識・候補・最終出力"; details.append(summary);
  for (const [name, text] of [["Whisper",result.raw], ["補正候補（棄却も含む）",result.candidate], ["最終出力（棄却時はWhisper原文）",result.finalText]]) {
    const label = document.createElement("p");label.textContent = name;
    const content = document.createElement("pre");content.textContent = text ?? "—";details.append(label,content);
  }
  const button = document.createElement("button");button.textContent = "最終出力をコピー";
  button.addEventListener("click",()=>void action(()=>invoke("copy_comparison_result",{generation,index:result.index})));
  details.append(button); td.append(details);
}
async function refresh() {
  if (reading || !isTauri()) return;
  reading = true;
  try {
    const next = await invoke<Status>("comparison_status", { generation, after: cursor });
    if (generation !== next.generation) {generation=next.generation;cursor=0;el("compare-rows").replaceChildren();}
    state = next;
    for (const row of next.rows) append(row);
    cursor=next.next;
    el<HTMLInputElement>("compare-arm").checked=next.armed;
    el<HTMLButtonElement>("compare-start").disabled=acting || next.working;
    el<HTMLButtonElement>("compare-cancel").disabled=acting || !next.working;
    el<HTMLInputElement>("compare-arm").disabled=acting || next.working;
    el<HTMLButtonElement>("compare-copy-report").disabled=acting || next.working || next.completed===0 || next.more;
    const diagnostic = next.foundation;
    el("compare-foundation").textContent = diagnostic
      ? `Apple FM: ${diagnostic.status?.availability ?? diagnostic.error ?? "未確認"} / 確認 ${seconds(diagnostic.elapsedMs)}${diagnostic.status ? ` / macOS ${diagnostic.status.os_major}.${diagnostic.status.os_minor}.${diagnostic.status.os_patch}` : ""}。利用可否は比較開始時点の結果で、生成成功の保証ではありません。`
      : "Apple FM: この比較では未確認。Apple比較プリセットを選び、録音済み音声で比較を開始してください。";
    el("compare-progress").textContent = `${next.phase}: ${next.completed}/${next.total} / 音声 ${seconds(next.audioMs)} / AX ${next.axCharacters}文字、OCR ${next.ocrCharacters}文字\n${next.message}`;
  } catch (error) { message(String(error)); }
  finally { reading=false; }
}
async function action(work: () => Promise<unknown>) {
  if (acting) return;
  acting=true; message("");
  try { await work(); } catch (error) {message(String(error));}
  finally {acting=false;await refresh();}
}
async function count() {
  if (!isTauri()) return;
  try {el("compare-count").textContent=`重複する入力条件を除き ${await invoke<number>("comparison_plan",{selection:selection()})} 件を直列に実行します。`;}
  catch (error) {el("compare-count").textContent=String(error);}
}
async function loadChoices() {
  choices = await invoke<Choice[]>("comparison_models");
  for (const [id, kind] of [["compare-speech","transcription"],["compare-correction","correction"]]) {
    const select=el<HTMLSelectElement>(id); const old=selected(id);select.replaceChildren();
    for (const choice of choices.filter(c=>c.kind===kind)) {
      const option=new Option(`${choice.name}${choice.available?"":"（未準備）"}`,choice.id);
      option.selected=old.includes(choice.id) || (!old.length && choice.id===(kind==="transcription"?"current-speech":"none"));
      select.add(option);
    }
  }
  await count();
}
function preset(correction: string) {
  if (acting || state?.working) { message("比較の終了を待ってから条件を変更してください。"); return; }
  const source = el<HTMLSelectElement>("compare-preset-source").value;
  for (const [id, values] of [
    ["compare-speech", ["current-speech"]],
    ["compare-correction", ["none", correction]],
    ["compare-sources", ["none", source]],
    ["compare-routes", ["none", "asr-only", "correction-only", "both"]],
  ] as [string, string[]][]) {
    for (const option of el<HTMLSelectElement>(id).options) option.selected=values.includes(option.value);
  }
  message("比較条件を選択しました。取得元の許可・保存と、録音前の比較用文脈保持が必要です。権限・設定・録音は変更していません。");
  void count();
}
export function setupComparison() {
  el("compare-open").addEventListener("click",()=>{const panel=el<HTMLDetailsElement>("comparison-panel");panel.open=!panel.open;});
  el("compare-start").addEventListener("click",()=>void action(()=>invoke("start_comparison",{selection:selection()})));
  el("compare-cancel").addEventListener("click",()=>void action(()=>invoke("cancel_comparison",{clear:false})));
  el("compare-clear").addEventListener("click",()=>void action(()=>invoke("cancel_comparison",{clear:true})));
  el("compare-arm").addEventListener("change",()=>void action(()=>invoke("arm_comparison_context",{enabled:el<HTMLInputElement>("compare-arm").checked})));
  el("compare-refresh-models").addEventListener("click",()=>void action(loadChoices));
  el("compare-preset-apple").addEventListener("click",()=>preset("apple-foundation"));
  el("compare-preset-qwen").addEventListener("click",()=>preset("current-correction"));
  el("compare-all").addEventListener("click",()=>{for (const id of ["compare-speech","compare-correction","compare-asr-backends","compare-correction-backends","compare-sources","compare-routes"]) for (const option of el<HTMLSelectElement>(id).options) option.selected=true;void count();});
  el("compare-copy-report").addEventListener("click",()=>void action(async()=>{await invoke("copy_comparison_report",{generation,includeText:el<HTMLInputElement>("compare-include-text").checked});message("比較レポートをコピーしました。");}));
  for (const input of el("comparison-panel").querySelectorAll("select,input,textarea")) input.addEventListener("change",()=>void count());
  if (isTauri()) {void action(loadChoices);void refresh();window.setInterval(()=>void refresh(),500);}
}

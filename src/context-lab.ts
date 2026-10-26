import { invoke, isTauri } from "@tauri-apps/api/core";
import storm from "../evaluation/context-evidence/assets/storm.png";
import candy from "../evaluation/context-evidence/assets/candy.png";

type LabRow = {scene:string;source:string;route:string;engine:string;instruction:string;repeat:number;status:string;raw?:string|null;candidate?:string|null;finalText?:string|null;judgment?:string;reason?:string;correctionMs?:number|null;asrMs?:number|null};
type LabStatus = {runId:string|null;working:boolean;phase:string;message:string;planned:number;completed:number;rows:LabRow[];next:number;more:boolean;parts:number;foundation:{status:{availability:string}|null;error:string|null}|null};
const el=<T extends HTMLElement>(id:string)=>document.getElementById(id) as T;
let state:LabStatus|undefined;
let cursor=0,part=0;
let reading=false,acting=false;
function setText(id:string,value:string){const node=el(id);if(node.textContent!==value)node.textContent=value;}
const append=(row:LabRow)=>{
  const tr=el<HTMLTableSectionElement>("lab-rows").insertRow();
  const ms=(n:number|null|undefined)=>n==null?"—":`${(n/1000).toFixed(2)}秒`;
  for(const text of [`${row.scene} / ${row.source} → ${row.route}\n${row.engine} / ${row.instruction} / #${row.repeat+1}`,`${row.status}\n${row.judgment??""}\n${row.reason??""}`,`ASR ${ms(row.asrMs)}\n補正 ${ms(row.correctionMs)}`,`原文: ${row.raw??"—"}\n候補: ${row.candidate??"—"}\n最終: ${row.finalText??"—"}`]) tr.insertCell().textContent=text;
};
function render(){
  const busy=acting||!!state?.working;
  el<HTMLButtonElement>("lab-start").disabled=!isTauri()||busy;
  el<HTMLButtonElement>("lab-cancel").disabled=acting||!state?.working;
  for(const id of ["lab-copy","lab-copy-part"]) el<HTMLButtonElement>(id).disabled=busy||!state?.runId||!!state.more;
  el<HTMLButtonElement>("lab-copy-part").disabled = busy || !state?.runId || !state.parts || !!state.more;
  el<HTMLButtonElement>("lab-clear").disabled = acting || !state?.runId;
  for(const id of ["lab-input","lab-apple","lab-qwen","lab-ax","lab-repeats","lab-instruction","lab-compare-instruction"]) (el(id) as HTMLInputElement).disabled=busy;
  setText("lab-progress",state?`${state.phase}: ${state.completed}/${state.planned}\n${state.message}\nAFM: ${state.foundation?.status?.availability??state.foundation?.error??"未確認"}`:"未実行。実行ボタンを押すまで取得・推論は行いません。");
  setText("lab-copy-part",`分割JSONをコピー（${part+1}/${Math.max(1,state?.parts??1)}）`);
}
async function refresh(){
  if(reading||!isTauri())return;reading=true;
  try{
    const next=await invoke<LabStatus>("context_lab_status",{runId:state?.runId??null,after:cursor});
    if(state?.runId!==next.runId){cursor=0;part=0;el<HTMLInputElement>("lab-include-text").checked=false;el("lab-rows").replaceChildren();el<HTMLTextAreaElement>("lab-json").value="";}
    state=next;for(const row of next.rows)append(row);cursor=next.next;render();
  }catch(error){el("lab-message").textContent=String(error);}
  finally{reading=false;}
}
async function action(fn:()=>Promise<unknown>){if(acting)return;acting=true;render();el("lab-message").textContent="";try{await fn();}catch(error){el("lab-message").textContent=String(error);}finally{acting=false;await refresh();render();}}
async function copy(partial:boolean){
  if(!state?.runId)return;
  const params={runId:state.runId,includeText:el<HTMLInputElement>("lab-include-text").checked,part:partial?part:null};
  const report=await invoke<string>("context_lab_report",params);
  el<HTMLTextAreaElement>("lab-json").value=report;
  await invoke("copy_context_lab_report",params);
  el("lab-message").textContent=partial?`分割 ${part+1}/${state.parts} をコピーしました。続きも順番にコピーしてください。`:"JSONをコピーしました。解析用には本文・文脈を含める選択が必要です。";
  if(partial)part=(part+1)%Math.max(1,state.parts);
}
export function setupContextLab(){
  el<HTMLImageElement>("lab-storm").src=storm;el<HTMLImageElement>("lab-candy").src=candy;
  el("lab-start").addEventListener("click",()=>void action(()=>invoke("start_context_lab",{options:{
    input:el<HTMLSelectElement>("lab-input").value,apple:el<HTMLInputElement>("lab-apple").checked,qwen:el<HTMLInputElement>("lab-qwen").checked,
    ax:el<HTMLInputElement>("lab-ax").checked,repeats:Number(el<HTMLSelectElement>("lab-repeats").value),compareInstruction:el<HTMLInputElement>("lab-compare-instruction").checked,instruction:el<HTMLTextAreaElement>("lab-instruction").value,
  }})));
  el("lab-cancel").addEventListener("click",()=>void action(()=>invoke("cancel_comparison",{clear:false})));
  el("lab-clear").addEventListener("click",()=>void action(async()=>{await invoke("cancel_comparison",{clear:true});cursor=0;part=0;el("lab-rows").replaceChildren();el<HTMLTextAreaElement>("lab-json").value="";}));
  el("lab-permission").addEventListener("click",()=>void action(()=>invoke("request_accessibility")));
  el("lab-copy").addEventListener("click",()=>void action(()=>copy(false)));
  el("lab-copy-part").addEventListener("click",()=>void action(()=>copy(true)));
  el("lab-include-text").addEventListener("change",()=>{part=0;el<HTMLTextAreaElement>("lab-json").value="";render();});
  render();if(isTauri()){void refresh();window.setInterval(()=>void refresh(),500);}
}

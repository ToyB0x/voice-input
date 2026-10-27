import { invoke, isTauri } from "@tauri-apps/api/core";
import storm from "../evaluation/context-evidence/assets/storm.png";
import rainfall from "../evaluation/context-evidence/assets/rainfall.png";
import candyShower from "../evaluation/context-evidence/assets/candy-shower.png";
import candy from "../evaluation/context-evidence/assets/candy.png";

type LabRow = {correctionMode?:string;scene:string;source:string;route:string;engine:string;instruction:string;repeat:number;status:string;raw?:string|null;candidate?:string|null;finalText?:string|null;judgment?:string;reason?:string;correctionMs?:number|null;asrMs?:number|null;inputForm?:string;correctionInput?:string|null;finalPass?:boolean};
type FinalGroup = {engine:string;inputForm:string;source:string;route:string;status:string;passed:number;required:number};
type LabStatus = {finalTestSummary?:FinalGroup[]|null;runId:string|null;working:boolean;phase:string;message:string;planned:number;completed:number;rows:LabRow[];next:number;more:boolean;parts:number;foundation:{status:{availability:string}|null;error:string|null}|null};
const el=<T extends HTMLElement>(id:string)=>document.getElementById(id) as T;
let state:LabStatus|undefined;
let cursor=0,part=0;
let reading=false,acting=false;
let message="";
let modelChoices:{id:string;available:boolean}[]=[];
function setMessage(value:string){message=value;setText("lab-message",value);}
function setText(id:string,value:string){const node=el(id);if(node.textContent!==value)node.textContent=value;}
const append=(row:LabRow)=>{
  const tr=el<HTMLTableSectionElement>("lab-rows").insertRow();
  const ms=(n:number|null|undefined)=>n==null?"—":`${(n/1000).toFixed(2)}秒`;
  for(const text of [`${row.scene} / ${row.source} → ${row.route}\n${row.engine} / ${row.correctionMode??"rewrite"} / ${row.inputForm??"original"} / ${row.instruction} / #${row.repeat+1}`,`${row.status}\n${row.judgment??""}\n${row.reason??""}${row.finalPass==null?"":`\n文全体: ${row.finalPass?"一致":"不一致"}`}`,`ASR ${ms(row.asrMs)}\n補正 ${ms(row.correctionMs)}`,`原文: ${row.raw??"—"}\n補正入力: ${row.correctionInput??"—"}\n候補: ${row.candidate??"—"}\n最終: ${row.finalText??"—"}`]) tr.insertCell().textContent=text;
};
function render(){
  const busy=acting||!!state?.working;
  el<HTMLButtonElement>("lab-start").disabled=!isTauri()||busy;
  el<HTMLButtonElement>("lab-cancel").disabled=acting||!state?.working;
  for(const id of ["lab-copy","lab-copy-part"]) el<HTMLButtonElement>(id).disabled=busy||!state?.runId||!!state.more;
  el<HTMLButtonElement>("lab-copy-part").disabled = busy || !state?.runId || !state.parts || !!state.more;
  el<HTMLButtonElement>("lab-clear").disabled = acting || !state?.runId;
  for(const id of ["lab-input","lab-apple","lab-qwen","lab-ax","lab-repeats","lab-instruction","lab-compare-instruction","lab-foundation-profile","lab-dataset","lab-input-form","lab-kana-hint","lab-final-preset","lab-qwen-mode","lab-qwen-dictionary","lab-join-ocr","lab-qwen-preset"]) (el(id) as HTMLInputElement).disabled=busy;
  const qwen=el<HTMLInputElement>("lab-qwen").checked;
  const speech=el<HTMLSelectElement>("lab-input").value!=="text";
  const ready=(id:string)=>modelChoices.find(x=>x.id===id)?.available===true;
  setText("lab-model-readiness",[
    speech?`Whisper: ${ready("current-speech")?"保存済みモデルあり":"未準備／保存済みパスの参照先なし"}`:"Whisper: 不要（かな原文のみ）",
    qwen?`Qwen: ${ready("current-correction")?"保存済みモデルあり":"未準備／保存済みパスの参照先なし。「モデル」で準備・検証 → 設定欄へ反映 → 設定を保存"}`:"Qwen: 比較しない",
  ].join("\n"));
  setText("lab-final-status",(state?.finalTestSummary??[]).filter(x=>["vision-labels","vision-regions"].includes(x.source)&&x.route==="correction").map(x=>`画像分類 ${x.source==="vision-regions"?"領域":"全体"}→補正 / ${x.engine} / ${x.inputForm}: ${x.status}（文全体 ${x.passed}/${x.required}）`).join("\n"));
  setText("lab-message",message);
  setText("lab-progress",state?`${state.phase}: ${state.completed}/${state.planned}\n${state.message}\nAFM: ${state.foundation?.status?.availability??state.foundation?.error??"未確認"}`:"未実行。実行ボタンを押すまで取得・推論は行いません。");
  setText("lab-copy-part",`分割JSONをコピー（${part+1}/${Math.max(1,state?.parts??1)}）`);
}
async function refresh(){
  if(reading||!isTauri())return;reading=true;
  try{
    const [next,models]=await Promise.all([
      invoke<LabStatus>("context_lab_status",{runId:state?.runId??null,after:cursor}),
      invoke<{id:string;available:boolean}[]>("comparison_models"),
    ]);
    modelChoices=models;
    if(state?.runId!==next.runId){cursor=0;part=0;el<HTMLInputElement>("lab-include-text").checked=false;el("lab-rows").replaceChildren();el<HTMLTextAreaElement>("lab-json").value="";}
    state=next;for(const row of next.rows)append(row);cursor=next.next;render();
  }catch(error){setMessage(String(error));}
  finally{reading=false;}
}
async function action(fn:()=>Promise<unknown>){
  if(acting)return;acting=true;setMessage("操作を確認中…");render();
  try{await fn();if(message==="操作を確認中…")setMessage("");}
  catch(error){setMessage(String(error));}
  finally{acting=false;await refresh();render();}
}
async function copy(partial:boolean){
  if(!state?.runId)return;
  const params={runId:state.runId,includeText:el<HTMLInputElement>("lab-include-text").checked,part:partial?part:null};
  const report=await invoke<string>("context_lab_report",params);
  el<HTMLTextAreaElement>("lab-json").value=report;
  await invoke("copy_context_lab_report",params);
  setMessage(partial?`分割 ${part+1}/${state.parts} をコピーしました。続きも順番にコピーしてください。`:"JSONをコピーしました。解析用には本文・文脈を含める選択が必要です。");
  if(partial)part=(part+1)%Math.max(1,state.parts);
}
function updatePictures(){
  const falling=el<HTMLSelectElement>("lab-dataset").value==="ame-falling-v1";
  const a=el<HTMLImageElement>("lab-storm"),b=el<HTMLImageElement>("lab-candy");
  a.src=falling?rainfall:storm;b.src=falling?candyShower:candy;
  a.alt=falling?"雨が降る住宅街の合成画像":"台風の合成画像";
  setText("lab-scene-a",falling?"降雨 → 期待文「雨が降っています」":"台風 → 期待語「雨」");
  setText("lab-scene-b",falling?"屋根からの飴まき → 期待文「飴が降っています」":"キャンディ → 期待語「飴」");
  b.alt=falling?"屋根から飴をまくお祭りの合成画像":"キャンディの合成画像";
}
export function setupContextLab(){
  updatePictures();
  el("lab-dataset").addEventListener("change",updatePictures);
  el("lab-final-preset").addEventListener("click",()=>{
    el<HTMLSelectElement>("lab-dataset").value="ame-falling-v1";
    el<HTMLSelectElement>("lab-input-form").value="both";
    el<HTMLSelectElement>("lab-input").value="bundled";
    el<HTMLSelectElement>("lab-foundation-profile").value="screen-reading-v3";
    el<HTMLInputElement>("lab-apple").checked=true;el<HTMLInputElement>("lab-qwen").checked=false;
    el<HTMLInputElement>("lab-kana-hint").checked=true;
    el<HTMLSelectElement>("lab-repeats").value="2";
    el<HTMLInputElement>("lab-compare-instruction").checked=false;
    el<HTMLTextAreaElement>("lab-instruction").value="";
    updatePictures();setMessage("雨／飴まき試験を設定しました。AXは現在の選択を維持します。開始ボタンで実行してください。");render();
  });
  el("lab-qwen-preset").addEventListener("click",()=>{
    el<HTMLSelectElement>("lab-dataset").value="ame-falling-v1";
    el<HTMLSelectElement>("lab-input-form").value="original";
    el<HTMLSelectElement>("lab-input").value="bundled";
    el<HTMLInputElement>("lab-apple").checked=false;el<HTMLInputElement>("lab-qwen").checked=true;
    el<HTMLSelectElement>("lab-qwen-mode").value="replacements";
    el<HTMLTextAreaElement>("lab-qwen-dictionary").value="あめ=雨,飴,あめ\nはし=橋,箸,はし\nこうしょう=交渉,校章,こうしょう";
    el<HTMLInputElement>("lab-kana-hint").checked=false;
    el<HTMLInputElement>("lab-join-ocr").checked=false;
    el<HTMLSelectElement>("lab-repeats").value="2";
    el<HTMLInputElement>("lab-compare-instruction").checked=false;
    el<HTMLTextAreaElement>("lab-instruction").value="";
    updatePictures();setMessage("Qwenの辞書候補選択を設定しました。保存済みのQwenモデルを使います。開始ボタンで実行してください。");render();
  });
  el("lab-start").addEventListener("click",()=>void action(()=>invoke("start_context_lab",{options:{
    qwenMode:el<HTMLSelectElement>("lab-qwen-mode").value,qwenDictionary:el<HTMLTextAreaElement>("lab-qwen-dictionary").value,joinOcrKatakana:el<HTMLInputElement>("lab-join-ocr").checked,
    dataset:el<HTMLSelectElement>("lab-dataset").value,inputForm:el<HTMLSelectElement>("lab-input-form").value,kanaHint:el<HTMLInputElement>("lab-kana-hint").checked,
    input:el<HTMLSelectElement>("lab-input").value,apple:el<HTMLInputElement>("lab-apple").checked,qwen:el<HTMLInputElement>("lab-qwen").checked,
    ax:el<HTMLInputElement>("lab-ax").checked,repeats:Number(el<HTMLSelectElement>("lab-repeats").value),compareInstruction:el<HTMLInputElement>("lab-compare-instruction").checked,instruction:el<HTMLTextAreaElement>("lab-instruction").value,foundationProfile:el<HTMLSelectElement>("lab-foundation-profile").value,
  }})));
  el("lab-cancel").addEventListener("click",()=>void action(()=>invoke("cancel_comparison",{clear:false})));
  el("lab-clear").addEventListener("click",()=>void action(async()=>{await invoke("cancel_comparison",{clear:true});cursor=0;part=0;el("lab-rows").replaceChildren();el<HTMLTextAreaElement>("lab-json").value="";}));
  el("lab-permission").addEventListener("click",()=>void action(()=>invoke("request_accessibility")));
  el("lab-copy").addEventListener("click",()=>void action(()=>copy(false)));
  el("lab-copy-part").addEventListener("click",()=>void action(()=>copy(true)));
  el("lab-include-text").addEventListener("change",()=>{part=0;el<HTMLTextAreaElement>("lab-json").value="";render();});
  for(const id of ["lab-input","lab-qwen"])el(id).addEventListener("change",()=>{render();void refresh();});
  render();if(isTauri()){void refresh();window.setInterval(()=>void refresh(),500);}
}

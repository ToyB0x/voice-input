import { invoke, isTauri } from "@tauri-apps/api/core";
import definitions from "./usage-presets.json";
type Preset = typeof definitions[number];
type Model = { id: string; name: string; bytes: number; cached: boolean; memoryHint?:string };
type Preparation = {generation:number;phase:string;message:string;received:number;total:number;index:number;count:number;prepared:{id:string;path:string}[]};
export function setupUsagePresets(locked:()=>boolean, apply:(id:string,generation:number)=>Promise<void>, current:()=>string|null) {
  const el=<T extends HTMLElement>(id:string)=>document.getElementById(id) as T;
  const choice=el<HTMLSelectElement>("usage-preset");
  let models:Model[]=[], state:Preparation|undefined;
  let acting=false, polling=false, ignored=false, loading=false;
  let message="プリセットを選び、モデルを準備してから保存してください。選択だけでは設定を変更しません。";
  const selected=():Preset=>definitions.find(p=>p.id===choice.value)??definitions[0];
  const ids=()=>[selected().speech,...selected().correction?[selected().correction!]:[]];
  const preparing=()=>!!state&&["starting","downloading","verifying"].includes(state.phase)&&!ignored;
  const matching=()=>!!state&&!ignored&&state.phase==="ready"&&state.prepared.map(x=>x.id).join()===ids().join();
  const text=(id:string,value:string)=>{const node=el(id);if(node.textContent!==value)node.textContent=value;};
  function render(){
    const p=selected(), busy=acting||preparing()||loading;
    choice.disabled=!isTauri()||busy||locked();
    el<HTMLButtonElement>("usage-prepare").disabled=!isTauri()||!models.length||busy||locked();
    el<HTMLButtonElement>("usage-apply").disabled=!matching()||busy||locked();
    el<HTMLButtonElement>("usage-cancel").disabled=acting||!preparing();
    const chosen=ids().map(id=>models.find(m=>m.id===id)).filter((m):m is Model=>!!m);
    const missing=chosen.filter(m=>!m.cached).reduce((n,m)=>n+m.bytes,0);
    text("usage-description",p.description);text("usage-evidence",p.evidence);
    text("usage-models",chosen.length?chosen.map(m=>`${m.name} / ${(m.bytes/1e9).toFixed(2)} GB / ${m.cached?"保存あり・使用前に検証":"未取得"}${m.memoryHint?`。${m.memoryHint}`:""}`).join("\n"):"モデル一覧を確認中…");
    text("usage-prepare",missing?`必要なモデルを準備（追加取得 最大 ${(missing/1e9).toFixed(2)} GB）`:"保存済みモデルを検証");
    text("usage-current",`保存済みのモード: ${definitions.find(p=>p.id===current())?.name??"個別設定"}`);
    text("usage-status",message);
    const progress=el<HTMLProgressElement>("usage-progress");progress.hidden=!preparing();progress.max=Math.max(state?.total??0,1);progress.value=state?.received??0;
  }
  function show(){
    if(!state)return;
    message=`${state.message}${preparing()?` ${state.index}/${state.count} · ${(state.received/1e6).toFixed(0)} / ${(state.total/1e6).toFixed(0)} MB`:""}`;
    if(state.phase==="ready")message="モデルの検証が完了しました。「設定を保存して読み込む」でこのプリセットを使えます。";
  }
  choice.addEventListener("change",()=>{state=undefined;ignored=false;message="選択を変更しました。モデルを準備・検証してください。";render();});
  el("usage-prepare").addEventListener("click",()=>void(async()=>{
    if(acting||preparing()||locked())return;
    acting=true;ignored=false;message="モデルの準備を開始しています…";render();
    try{state=await invoke<Preparation>("prepare_models",{ids:ids(),download:true});show();}
    catch(error){message=String(error);}
    finally{acting=false;render();}
  })());
  el("usage-cancel").addEventListener("click",()=>void(async()=>{
    ignored=true;message="準備を取り消しました。設定は変更していません。";render();
    try{await invoke("cancel_model_preparation");}catch(error){message=String(error);render();}
  })());
  el("usage-apply").addEventListener("click",()=>void(async()=>{
    if(!matching()||acting||locked())return;
    acting=true;message="設定を保存しています…";render();
    try{await apply(selected().id,state!.generation);loading=true;message="設定を保存しました。音声モデルを読み込み中…";}
    catch(error){message=String(error);}
    finally{acting=false;render();}
  })());
  async function poll(){
    if(polling||acting||!isTauri())return;polling=true;
    try{
      if(loading){
        const recording=await invoke<{working:boolean;modelReady:boolean;recognitionError:string|null}>("recording_status");
        if(!recording.working){loading=false;message=recording.modelReady?"準備と設定が完了しました。「音声入力」で録音できます。":"設定は保存済みですが、モデルを読み込めませんでした。モデル欄のエラーを確認し、再読み込みしてください。";}
      }
      if(state&&!ignored){
        const next=await invoke<Preparation>("model_preparation_status");
        if(next.generation!==state.generation){ignored=true;message="別のモデル準備が行われました。このプリセットをもう一度検証してください。";}
        else{const changed=next.phase!==state.phase||next.received!==state.received||next.message!==state.message;state=next;if(changed&&!loading)show();}
      }
      models=await invoke<Model[]>("model_catalog");render();
    }catch(error){message=String(error);render();}finally{polling=false;}
  }
  choice.value=definitions[0].id;render();if(isTauri()){void poll();window.setInterval(()=>void poll(),500);}
  return {refreshControls:render};
}

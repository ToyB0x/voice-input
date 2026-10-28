import { invoke, isTauri } from '@tauri-apps/api/core';
import config from '../evaluation/ame-lt/scenarios.json';
import audio from '../evaluation/context-evidence/baseline/fixtures/speech.wav?url';
import rain from '../evaluation/context-evidence/assets/rainfall.png';
import candy from '../evaluation/context-evidence/assets/candy-shower.png';
type Row = {stage: string; label: string; repeat?: number; raw: string; final: string; status: string; observation?: string; error?: string; elapsedMs: number};
type State = {working: boolean; mode: string; phase: string; message: string; raw: string | null; rows: Row[]; completed: number; total: number};
type Model = {id: string; cached: boolean};
type Preparation = {phase: string; message: string; received: number; total: number; modelId: string};
export function setupDemo() {
  const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
  el<HTMLAudioElement>('demo-audio').src = audio;
  el<HTMLImageElement>('demo-rain').src = rain;
  el<HTMLImageElement>('demo-candy').src = candy;
  el<HTMLTextAreaElement>('demo-weather').value = config.texts[0].text;
  el<HTMLTextAreaElement>('demo-halloween').value = config.texts[1].text;
  let state: State | undefined, models: Model[] = [], preparing = false, requesting = false, polling = false, lastRows = '';
  const busy = () => requesting || preparing || !!state?.working;
  const cached = (id: string) => models.some(m => m.id === id && m.cached);
  function render() {
    for (const id of ['demo-prepare', 'demo-prepare-audio']) el<HTMLButtonElement>(id).disabled = !isTauri() || busy();
    for (const stage of ['audio','text','image']) el<HTMLButtonElement>(`demo-run-${stage}`).disabled = !isTauri() || busy() || !cached('large-v3') || (stage !== 'audio' && !cached('demo-vlm')) || (stage === 'image' && !cached('demo-projector'));
    for (const id of ['demo-weather','demo-halloween','demo-repeats']) el<HTMLTextAreaElement | HTMLSelectElement>(id).disabled = busy();
    el<HTMLButtonElement>('demo-cancel').disabled = requesting || !state?.working || state.mode !== 'demo';
    el<HTMLButtonElement>('demo-cancel-prepare').disabled = requesting || !preparing;
    el<HTMLButtonElement>('demo-clear').disabled = busy() || !state?.rows.length;
    el<HTMLButtonElement>('demo-export').disabled = busy() || !state?.rows.length;
    el('demo-status').textContent = state?.mode === 'demo' ? `${state.message} (${state.completed}/${state.total})` : state?.working ? '別の比較・検証が実行中です。' : '準備ができたら①から始めてください。';
    el('demo-baseline').textContent = state?.raw ? `共通の認識結果：${state.raw}` : '';
    const rows = JSON.stringify(state?.rows ?? []);
    if (lastRows !== rows) {
      lastRows = rows;
      el('demo-results').replaceChildren(...(state?.rows ?? []).map(row => {
        const article = document.createElement('article'); article.className = 'demo-result';
        const title = document.createElement('h4'); title.textContent = row.label + (row.repeat === undefined ? '' : `・${row.repeat + 1}回目`);
        const result = document.createElement('p'); result.className = 'demo-output'; result.textContent = row.final;
        const detail = document.createElement('p'); detail.textContent = `${row.status === 'error' ? '失敗・原文保持' : row.status === 'undetermined' ? '判断できず原文保持' : '実測結果'}／${(row.elapsedMs / 1000).toFixed(1)}秒`;
        article.append(title,result,detail);
        if (row.observation) {const p=document.createElement('p');p.textContent=`画像からの観察：${row.observation}`;article.append(p);}
        if (row.error) {const p=document.createElement('p');p.textContent=row.error;article.append(p);}
        return article;
      }));
    }
  }
  async function poll() {
    if (!isTauri() || polling) return;
    polling = true;
    try {
      const [next,catalog,prep] = await Promise.all([invoke<State>('demo_status'),invoke<Model[]>('model_catalog'),invoke<Preparation>('model_preparation_status')]);
      state=next;models=catalog;preparing=['starting','downloading','verifying'].includes(prep.phase);
      el('demo-model-status').textContent = preparing ? `${prep.message} ${(prep.received/1e9).toFixed(2)} / ${(prep.total/1e9).toFixed(2)} GB` : `音声：${cached('large-v3')?'保存あり':'未準備'}／文章・画像モデル：${cached('demo-vlm')?'保存あり':'未準備'}／画像エンコーダー：${cached('demo-projector')?'保存あり':'未準備'}。実行前にSHA-256を検証します。${['error','cancelled'].includes(prep.phase)?prep.message:''}`;
      const progress=el<HTMLProgressElement>('demo-progress');progress.hidden=!preparing;progress.max=Math.max(1,prep.total);progress.value=prep.received;
    } catch(e) {el('demo-error').textContent=String(e);} finally {polling=false;render();}
  }
  async function action(fn: () => Promise<unknown>) {
    if (requesting) return;
    requesting=true;el('demo-error').textContent='';render();
    try {await fn();} catch(e) {el('demo-error').textContent=String(e);} finally {requesting=false;await poll();render();}
  }
  for (const stage of ['audio','text','image']) el(`demo-run-${stage}`).addEventListener('click',()=>void action(()=>invoke('start_demo',{options:{stage,repeats:Number(el<HTMLSelectElement>('demo-repeats').value),texts:[el<HTMLTextAreaElement>('demo-weather').value,el<HTMLTextAreaElement>('demo-halloween').value]}})));
  el('demo-prepare').addEventListener('click',()=>void action(()=>invoke('prepare_models',{ids:['large-v3','demo-vlm','demo-projector'],download:true})));
  el('demo-prepare-audio').addEventListener('click',()=>void action(()=>invoke('prepare_models',{ids:['large-v3'],download:true})));
  el('demo-cancel-prepare').addEventListener('click',()=>void action(()=>invoke('cancel_model_preparation')));
  el('demo-cancel').addEventListener('click',()=>void action(()=>invoke('cancel_comparison',{clear:false})));
  el('demo-clear').addEventListener('click',()=>void action(()=>invoke('clear_demo')));
  el('demo-export').addEventListener('click',()=>{
    if (!state || busy()) return;
    const blob=new Blob([JSON.stringify(state,null,2)],{type:'application/json'}),url=URL.createObjectURL(blob),a=document.createElement('a');
    a.href=url;a.download='ame-demo-results.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  });
  if (!isTauri()) el('demo-model-status').textContent='実行にはMacアプリが必要です。ブラウザーでは画面と同梱音声だけ確認できます。';
  render();void poll();setInterval(()=>void poll(),1500);
}

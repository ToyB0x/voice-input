import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';
const definitions=JSON.parse(readFileSync(new URL('../../src/usage-presets.json',import.meta.url),'utf8'));
async function setup({cached=false,phase='ready'}={}) {
  const nodes=new Map();
  for(const [,id] of readFileSync(new URL('../../index.html',import.meta.url),'utf8').matchAll(/id="([^"]+)"/g))nodes.set(id,{value:'',disabled:false,hidden:false,textContent:'',listeners:new Map(),addEventListener(type,fn){this.listeners.set(type,fn);}});
  const state={generation:1,phase,message:phase==='error'?'SHA-256が一致しません':'検証完了',received:0,total:100,index:1,count:1,prepared:[]};
  const requests=[],applied=[];let poll,active=null;
  const context=vm.createContext({document:{getElementById:id=>nodes.get(id)},window:{setInterval(fn){poll=fn;}}});
  const stub=values=>new vm.SyntheticModule(Object.keys(values),function(){for(const [k,v]of Object.entries(values))this.setExport(k,v);},{context});
  const api=stub({isTauri:()=>true,invoke:async(command,args)=>{
    requests.push({command,args:structuredClone(args)});
    if(command==='model_catalog')return [{id:'large-v3-turbo',name:'Whisper turbo',bytes:1624555275,cached},{id:'qwen3-4b',name:'Qwen4B',bytes:2497280256,cached}];
    if(command==='prepare_models'){state.prepared=args.ids.map(id=>({id,path:`/models/${id}`}));return structuredClone(state);}
    if(command==='model_preparation_status')return structuredClone(state);
    if(command==='recording_status')return {working:false,modelReady:true,recognitionError:null};
    if(command==='cancel_model_preparation')return;
    throw new Error(command);
  }});
  const module=new vm.SourceTextModule(stripTypeScriptTypes(readFileSync(new URL('../../src/usage-presets.ts',import.meta.url),'utf8')),{context});
  await module.link(name=>name==='@tauri-apps/api/core'?api:stub({default:definitions}));await module.evaluate();
  const controls=module.namespace.setupUsagePresets(()=>false,async(id,generation)=>{applied.push({id,generation});active=id;},()=>active);
  const settle=()=>new Promise(setImmediate);await settle();
  return {nodes,state,requests,applied,controls,settle,click:async id=>{nodes.get(id).listeners.get('click')();await settle();},choose:async id=>{nodes.get('usage-preset').value=id;nodes.get('usage-preset').listeners.get('change')();await settle();},poll:async()=>{poll();await settle();}};
}
test('opening and selecting presets do not download, save, load or request permission',async()=>{
  const a=await setup();await a.choose('qwen-preview');
  assert.ok(a.requests.every(x=>x.command==='model_catalog'));
  assert.match(a.nodes.get('usage-evidence').textContent,/比較/);
  assert.equal(a.nodes.get('usage-apply').disabled,true);
});
test('prepare verifies cached models and explicit apply completes setup without a separate save button',async()=>{
  const a=await setup({cached:true});assert.equal(a.nodes.get('usage-prepare').textContent,'保存済みモデルを検証');
  await a.click('usage-prepare');const request=a.requests.find(x=>x.command==='prepare_models');assert.deepEqual([...request.args.ids],['large-v3-turbo']);
  assert.equal(a.applied.length,0);assert.equal(a.nodes.get('usage-apply').disabled,false);
  await a.click('usage-apply');assert.deepEqual(a.applied,[{id:'asr-first',generation:1}]);await a.poll();
  assert.match(a.nodes.get('usage-status').textContent,/準備と設定が完了/);
  assert.match(a.nodes.get('usage-current').textContent,/原文重視/);
});
test('Qwen preview prepares both models and stale generations cannot be applied',async()=>{
  const a=await setup();await a.choose('qwen-preview');await a.click('usage-prepare');
  assert.deepEqual([...a.requests.find(x=>x.command==='prepare_models').args.ids],['large-v3-turbo','qwen3-4b']);
  a.state.generation=2;await a.poll();assert.equal(a.nodes.get('usage-apply').disabled,true);
  await a.click('usage-apply');assert.equal(a.applied.length,0);
  assert.match(a.nodes.get('usage-status').textContent,/もう一度検証/);
});
test('cancel and failed verification never apply settings even if a late ready result arrives',async()=>{
  const a=await setup({phase:'downloading'});await a.click('usage-prepare');await a.click('usage-cancel');a.state.phase='ready';await a.poll();
  assert.equal(a.nodes.get('usage-apply').disabled,true);assert.equal(a.applied.length,0);
  const b=await setup({phase:'error'});await b.click('usage-prepare');assert.match(b.nodes.get('usage-status').textContent,/SHA-256/);assert.equal(b.nodes.get('usage-apply').disabled,true);
});
test('unchanged polling preserves the prepare button native text node',async()=>{
  const a=await setup();const button=a.nodes.get('usage-prepare');let value=button.textContent,writes=0;
  Object.defineProperty(button,'textContent',{get(){return value;},set(v){value=v;writes++;}});
  await a.poll();a.controls.refreshControls();await a.poll();assert.equal(writes,0);
});

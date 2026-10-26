import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';

async function setup(){
  const elements=new Map();
  for(const [,id] of readFileSync(new URL('../../index.html',import.meta.url),'utf8').matchAll(/id="([^"]+)"/g)){
    elements.set(id,{value:'',checked:false,disabled:false,textContent:'',listeners:new Map(),rows:[],
      addEventListener(k,fn){this.listeners.set(k,fn);},replaceChildren(){this.rows=[];},
      insertRow(){const row={cells:[],insertCell(){const c={textContent:''};this.cells.push(c);return c;}};this.rows.push(row);return row;}});
  }
  elements.get('lab-input').value='text';elements.get('lab-repeats').value='1';elements.get('lab-apple').checked=true;elements.get('lab-compare-instruction').checked=true;
  const state={runId:null,working:false,phase:'idle',message:'',planned:0,completed:0,rows:[],next:0,more:false,parts:0,foundation:null};
  const requests=[];let poll,startError;
  const models=[{id:"current-speech",available:false},{id:"current-correction",available:false}];
  const context=vm.createContext({document:{getElementById:id=>elements.get(id)},window:{setInterval(fn){poll=fn;}}});
  const stub=exports=>new vm.SyntheticModule(Object.keys(exports),function(){for(const [k,v] of Object.entries(exports))this.setExport(k,v);},{context});
  const api=stub({isTauri:()=>true,invoke:async(command,args)=>{requests.push({command,args:structuredClone(args)});
    if(command==='context_lab_status')return structuredClone(state);
    if(command==='comparison_models')return structuredClone(models);
    if(command==='start_context_lab' && startError)throw startError;
    if(command==='context_lab_report')return JSON.stringify({includesText:args.includeText,part:args.part});
    if(command==='cancel_comparison'){state.runId=null;state.rows=[];state.completed=0;}
  }});
  const source=stripTypeScriptTypes(readFileSync(new URL('../../src/context-lab.ts',import.meta.url),'utf8'));
  const module=new vm.SourceTextModule(source,{context});await module.link(name=>name==='@tauri-apps/api/core'?api:stub({default:'synthetic.png'}));await module.evaluate();
  module.namespace.setupContextLab();const settle=()=>new Promise(setImmediate);await settle();
  return {elements,state,requests,models,setStartError(value){startError=value;},settle,poll:async()=>{poll();await settle();},click:async id=>{elements.get(id).listeners.get('click')();await settle();}};
}
test('opening the lab does not start capture, request permission or enable text sharing',async()=>{
  const a=await setup();assert.ok(a.requests.every(x=>['context_lab_status','comparison_models'].includes(x.command)));
  assert.equal(a.elements.get('lab-include-text').checked,false);
  await a.click('lab-start');const r=a.requests.find(x=>x.command==='start_context_lab');assert.equal(r.args.options.input,'text');assert.equal(r.args.options.apple,true);assert.equal(r.args.options.qwen,false);
  assert.ok(!a.requests.some(x=>x.command==='request_accessibility'));
});
test('copy is opt in and pages advance only on explicit actions',async()=>{
  const a=await setup();Object.assign(a.state,{runId:'run',phase:'done',completed:25,planned:25,parts:2});await a.poll();
  await a.click('lab-copy');let r=a.requests.filter(x=>x.command==='copy_context_lab_report').at(-1);assert.equal(r.args.includeText,false);assert.equal(r.args.part,null);
  a.elements.get('lab-include-text').checked=true;
  await a.click('lab-copy-part');await a.click('lab-copy-part');const copies=a.requests.filter(x=>x.command==='copy_context_lab_report').slice(-2);
  assert.deepEqual(copies.map(x=>x.args.part),[0,1]);assert.ok(copies.every(x=>x.args.includeText));
  assert.equal(JSON.parse(a.elements.get('lab-json').value).part,1);
  a.state.runId='new-run';await a.poll();assert.equal(a.elements.get('lab-include-text').checked,false);assert.equal(a.elements.get('lab-json').value,'');
});

test('polling preserves the copy button text node while the label is unchanged',async()=>{
  const a=await setup();const node=a.elements.get('lab-copy-part');let value=node.textContent,writes=0;
  Object.defineProperty(node,'textContent',{get(){return value;},set(v){value=v;writes++;}});
  await a.poll();await a.poll();assert.equal(writes,0);
  a.state.runId='new';a.state.parts=2;await a.poll();assert.equal(writes,1);
});

test('missing saved Qwen readiness and the start error remain visible across status polls',async()=>{
  const a=await setup();a.elements.get('lab-qwen').checked=true;await a.poll();
  assert.match(a.elements.get('lab-model-readiness').textContent,/Qwen: 未準備/);
  a.setStartError('保存済みのQwenモデルが見つかりません。設定を保存してください。');
  await a.click('lab-start');const message=a.elements.get('lab-message').textContent;
  assert.match(message,/Qwenモデル/);await a.poll();await a.poll();
  assert.equal(a.elements.get('lab-message').textContent,message);
  assert.equal(a.elements.get('lab-start').disabled,false);
  a.models[1].available=true;await a.poll();assert.match(a.elements.get('lab-model-readiness').textContent,/Qwen: 保存済みモデルあり/);
  assert.equal(a.elements.get('lab-message').textContent,message);
  a.setStartError(undefined);await a.click('lab-start');assert.equal(a.elements.get('lab-message').textContent,'');
});

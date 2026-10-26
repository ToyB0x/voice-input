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
  const requests=[];let poll;
  const context=vm.createContext({document:{getElementById:id=>elements.get(id)},window:{setInterval(fn){poll=fn;}}});
  const stub=exports=>new vm.SyntheticModule(Object.keys(exports),function(){for(const [k,v] of Object.entries(exports))this.setExport(k,v);},{context});
  const api=stub({isTauri:()=>true,invoke:async(command,args)=>{requests.push({command,args:structuredClone(args)});
    if(command==='context_lab_status')return structuredClone(state);
    if(command==='context_lab_report')return JSON.stringify({includesText:args.includeText,part:args.part});
    if(command==='cancel_comparison'){state.runId=null;state.rows=[];state.completed=0;}
  }});
  const source=stripTypeScriptTypes(readFileSync(new URL('../../src/context-lab.ts',import.meta.url),'utf8'));
  const module=new vm.SourceTextModule(source,{context});await module.link(name=>name==='@tauri-apps/api/core'?api:stub({default:'synthetic.png'}));await module.evaluate();
  module.namespace.setupContextLab();const settle=()=>new Promise(setImmediate);await settle();
  return {elements,state,requests,settle,poll:async()=>{poll();await settle();},click:async id=>{elements.get(id).listeners.get('click')();await settle();}};
}
test('opening the lab does not start capture, request permission or enable text sharing',async()=>{
  const a=await setup();assert.ok(a.requests.every(x=>x.command==='context_lab_status'));
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

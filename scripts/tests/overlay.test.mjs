import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';
async function setup() {
  const elements = Object.fromEntries(['root','phase','timing','meter','fill'].map(id=>[id,{hidden:true,textContent:'',dataset:{},style:{},attributes:new Map(),setAttribute(name,value){this.attributes.set(name,value);}}]));
  const requests=[];
  const module=new vm.SourceTextModule(stripTypeScriptTypes(readFileSync(new URL('../../src/overlay-view.ts',import.meta.url),'utf8')),{context:vm.createContext({})});
  await module.link(()=>{throw new Error('unexpected import');});await module.evaluate();
  const view=module.namespace.setupOverlayView(elements,()=>new Promise((resolve,reject)=>requests.push({resolve,reject})));
  return {elements,requests,view};
}
const recording={visible:true,phase:'recording',message:'録音中',recordingMs:1234,stageMs:0,level:0.01};
test('overlay polls do not overlap, and the speech level drives a visible accessible fill',async()=>{
  const app=await setup();const first=app.view.poll();await app.view.poll();
  assert.equal(app.requests.length,1);
  app.requests[0].resolve(recording);await first;
  assert.equal(app.elements.root.hidden,false);
  assert.match(app.elements.timing.textContent,/1.2秒/);
  assert.ok(Math.abs(parseFloat(app.elements.fill.style.width)-100/3)<0.001);
  assert.equal(app.elements.meter.attributes.get('aria-valuetext'),'-40.0 dBFS');
});
test('recognition and correction show elapsed time and zero the recording meter; hidden state clears it',async()=>{
  const app=await setup();
  for (const [index,phase] of ['recognizing','correcting'].entries()) {
    const work=app.view.poll();app.requests[index].resolve({...recording,phase,stageMs:2450});await work;
    assert.match(app.elements.timing.textContent,/2.5秒/);
    assert.equal(app.elements.fill.style.width,'0%');
  }
  const work=app.view.poll();app.requests[2].resolve({...recording,visible:false});await work;
  assert.equal(app.elements.root.hidden,true);assert.equal(app.elements.fill.style.width,'0%');
});
test('transport failures do not display exception content or keep a stale level',async()=>{
  const app=await setup();const work=app.view.poll();app.requests[0].reject(new Error('PRIVATE_PATH_OR_TEXT'));await work;
  assert.match(app.elements.phase.textContent,/設定画面を確認/);
  assert.doesNotMatch(app.elements.phase.textContent,/PRIVATE/);
  assert.equal(app.elements.fill.style.width,'0%');
  const next=app.view.poll();app.requests[1].resolve(recording);await next;
  assert.equal(app.elements.phase.textContent,'録音中');
});

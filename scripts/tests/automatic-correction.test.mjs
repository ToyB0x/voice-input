import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';

async function setup() {
  const elements = new Map();
  for (const [, id] of readFileSync(new URL('../../index.html', import.meta.url), 'utf8').matchAll(/id="([^"]+)"/g)) {
    elements.set(id, { value: '', checked: false, textContent: '', disabled: false, hidden: false, listeners: new Map(),
      addEventListener(type, callback) { this.listeners.set(type, callback); } });
  }
  const state = {
    snapshot: { phase: 'recorded', permission: 'authorized', elapsedMs: 1000, level: 0, sampleCount: 16000, sampleRate: 16000, inputDevice: null,
      modelReady: true, backend: 'metal', working: false, recognitionPhase: 'idle', transcript: 'Whisper原文', finalText: null, finalSource: 'correcting', recognitionError: null },
    desktop: { preferences: { version: 1, modelPath: '/model.bin', backend: 'metal', shortcut: 'Control+Alt+KeyV', autoInsert: false, autoCorrection: false, reuseCorrectionModel: false, showOverlay: false,
      recognitionContext: false, ocrContext: false, whisperContext: true, correctionContext: false, correctionModelPath: '', correctionEngine: 'apple-foundation', correctionBackend: 'metal' }, shortcutRegistered: true, error: null },
    correction: { working: true, checkingFoundation: false, foundation: null, text: null, error: null, elapsedMs: 0 },
  };
  let poll, applyUsage;
  const requests = [];
  const context = vm.createContext({ document: { querySelector: id => elements.get(id.slice(1)) }, window: { setInterval(callback) { poll = callback; } } });
  function stub(exports) { return new vm.SyntheticModule(Object.keys(exports), function () { for (const [name, value] of Object.entries(exports)) this.setExport(name, value); }, { context }); }
  const api = stub({ isTauri: () => true, invoke: async (command, args) => {
    requests.push({ command, args: args && structuredClone(args) });
    if (command === 'recording_status') return structuredClone(state.snapshot);
    if (command === 'desktop_status') return structuredClone(state.desktop);
    if (command === 'correction_status') return structuredClone(state.correction);
    if (command === 'insertion_status') return { enabled: false };
    if (command === 'context_status') return { enabled: false, screenPermitted: false };
    if (command === 'apply_usage_preset') {
      state.desktop.preferences={...structuredClone(args.preferences),usagePreset:args.id,modelPath:'/verified/large-v3-turbo.bin',backend:'metal',autoCorrection:false,recognitionContext:false,ocrContext:false,correctionContext:false,correctionInstruction:''};
      return structuredClone(state.desktop);
    }
    if (command === 'load_model') return structuredClone(state.snapshot);
    if (command === 'save_preferences') { state.desktop.preferences = structuredClone(args.preferences); return structuredClone(state.desktop); }
  } });
  const modules = {
    '@tauri-apps/api/core': api,
    './usage-presets': stub({ setupUsagePresets: (_locked, apply) => { applyUsage=apply; return { refreshControls() {} }; } }),
    './demo': stub({ setupDemo() {} }),
    './context-lab': stub({ setupContextLab() {} }),
    './comparison': stub({ setupComparison() {}, comparisonWorking: () => false }),
    './updates': stub({ setupUpdates() {}, updateInstalling: () => false }),
    './model-setup': stub({ setupModels: () => ({ refreshControls() {} }) }),
    './input-level': stub({ setupInputLevel: () => ({ setRecording() {} }) }),
    './styles.css': stub({}),
  };
  const source = stripTypeScriptTypes(readFileSync(new URL('../../src/main.ts', import.meta.url), 'utf8'));
  const module = new vm.SourceTextModule(source, { context });
  await module.link(name => modules[name]); await module.evaluate();
  const settle = () => new Promise(setImmediate); await settle();
  return { elements, state, requests, applyUsage: (...args) => applyUsage(...args), settle, poll: () => poll(), click: id => elements.get(id).listeners.get('click')?.() };
}

test('automatic correction is initially off and saved only by explicit preference action', async () => {
  const app = await setup();
  assert.equal(app.elements.get('auto-correction').checked, false);
  assert.equal(app.elements.get('show-overlay').checked, false);
  assert.ok(app.requests.every(r => r.command.endsWith('_status')));
  app.elements.get('auto-correction').checked = true;
  app.elements.get('show-overlay').checked = true;
  app.elements.get('correction-instruction').value = '自然な漢字表記にしてください。';
  app.click('save-preferences'); await app.settle();
  const saved = app.requests.find(r => r.command === 'save_preferences').args.preferences;
  assert.equal(saved.autoCorrection, true);
  assert.equal(saved.showOverlay, true);
  assert.equal(saved.correctionInstruction, '自然な漢字表記にしてください。');
  assert.equal(saved.autoInsert, false);
  assert.equal(saved.correctionEngine, 'apple-foundation');
});

test('pending final result cannot be copied; new recording and discard remain available', async () => {
  const app = await setup();
  assert.equal(app.elements.get('copy-final').disabled, true);
  assert.equal(app.elements.get('start').disabled, false);
  assert.equal(app.elements.get('clear').disabled, false);
  assert.match(app.elements.get('final-status').textContent, /補正完了まで待機/);
  app.state.snapshot.finalSource = 'fallback'; app.state.snapshot.finalText = 'Whisper原文'; app.state.correction.working = false;
  app.poll(); await app.settle();
  assert.equal(app.elements.get('final-text').value, 'Whisper原文');
  assert.equal(app.elements.get('transcript').value, 'Whisper原文');
  assert.equal(app.elements.get('copy-final').disabled, false);
  app.click('copy-final'); await app.settle();
  assert.equal(app.requests.filter(r => r.command === 'copy_final_transcript').length, 1);
  assert.ok(!app.requests.some(r => r.command === 'correct_transcript' || r.command === 'start_recording'));
});


test('reuse is opt in and per-stage timing distinguishes zero from unavailable', async () => {
  const app = await setup();
  assert.equal(app.elements.get('reuse-correction-model').checked, false);
  app.elements.get('correction-engine').value = 'qwen3';
  app.state.snapshot.recognitionMs = 250;
  app.state.correction.timings = { totalMs: 120, modelReused: true, backendMs: 0, modelLoadMs: 0,
    contextMs: 3, prefillMs: 10, generationMs: 100, validationMs: 1 };
  app.poll(); await app.settle();
  assert.equal(app.elements.get('reuse-correction-model').disabled, false);
  const text = app.elements.get('processing-timings').textContent;
  assert.match(text, /音声認識: 0.25秒/);
  assert.match(text, /モデル: 再利用/);
  assert.match(text, /読込: 0.00秒/);
  app.elements.get('reuse-correction-model').checked = true;
  app.click('save-preferences'); await app.settle();
  assert.equal(app.requests.find(r => r.command === 'save_preferences').args.preferences.reuseCorrectionModel, true);
  app.state.correction.timings = { totalMs: 50, modelReused: false };
  app.poll(); await app.settle();
  assert.match(app.elements.get('processing-timings').textContent, /読込: 未計測/);
});

test('preset application saves current unrelated fields and loads the verified model without another save action',async()=>{
  const app=await setup();app.elements.get('shortcut').value='Control+Alt+KeyB';
  app.elements.get('auto-insert').checked=true;
  await app.applyUsage('asr-first',7);
  const applied=app.requests.find(x=>x.command==='apply_usage_preset');
  assert.equal(applied.args.preferences.shortcut,'Control+Alt+KeyB');assert.equal(applied.args.preferences.autoInsert,true);
  const loaded=app.requests.find(x=>x.command==='load_model');
  assert.equal(loaded.args.path,'/verified/large-v3-turbo.bin');assert.equal(loaded.args.backend,'metal');
  assert.equal(app.elements.get('model-path').value,'/verified/large-v3-turbo.bin');
  assert.equal(app.elements.get('auto-correction').checked,false);
  assert.equal(app.requests.filter(x=>x.command==='save_preferences').length,0);
});

test('saving unchanged preset settings retains its name and changing a preset field returns to custom',async()=>{
  const app=await setup();await app.applyUsage('asr-first',7);
  app.click('save-preferences');await app.settle();
  assert.equal(app.requests.filter(x=>x.command==='save_preferences').at(-1).args.preferences.usagePreset,'asr-first');
  app.elements.get('model-path').value='/manual/model.bin';
  app.click('save-preferences');await app.settle();
  assert.equal(app.requests.filter(x=>x.command==='save_preferences').at(-1).args.preferences.usagePreset,null);
});


test('clipboard fallback defaults off for legacy preferences and is saved explicitly', async () => {
  const app = await setup();
  assert.equal(app.elements.get('clipboard-fallback').checked, false);
  app.elements.get('clipboard-fallback').checked = true;
  app.elements.get('clipboard-fallback').listeners.get('input')();
  app.state.correction.working = false;
  app.state.snapshot.finalSource = 'raw';
  app.poll(); await app.settle();
  app.click('save-preferences'); await app.settle();
  const saved = app.requests.find(r => r.command === 'save_preferences').args.preferences;
  assert.equal(saved.clipboardFallback, true);
});

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
    desktop: { preferences: { version: 1, modelPath: '/model.bin', backend: 'metal', shortcut: 'Control+Alt+KeyV', autoInsert: false, autoCorrection: false, showOverlay: false,
      recognitionContext: false, ocrContext: false, whisperContext: true, correctionContext: false, correctionModelPath: '', correctionEngine: 'apple-foundation', correctionBackend: 'metal' }, shortcutRegistered: true, error: null },
    correction: { working: true, checkingFoundation: false, foundation: null, text: null, error: null, elapsedMs: 0 },
  };
  let poll;
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
    if (command === 'save_preferences') { state.desktop.preferences = structuredClone(args.preferences); return structuredClone(state.desktop); }
  } });
  const modules = {
    '@tauri-apps/api/core': api,
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
  return { elements, state, requests, settle, poll: () => poll(), click: id => elements.get(id).listeners.get('click')?.() };
}

test('automatic correction is initially off and saved only by explicit preference action', async () => {
  const app = await setup();
  assert.equal(app.elements.get('auto-correction').checked, false);
  assert.equal(app.elements.get('show-overlay').checked, false);
  assert.ok(app.requests.every(r => r.command.endsWith('_status')));
  app.elements.get('auto-correction').checked = true;
  app.elements.get('show-overlay').checked = true;
  app.click('save-preferences'); await app.settle();
  const saved = app.requests.find(r => r.command === 'save_preferences').args.preferences;
  assert.equal(saved.autoCorrection, true);
  assert.equal(saved.showOverlay, true);
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

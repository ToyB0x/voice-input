import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';

// Exercise the actual frontend module with controlled IPC scheduling. No model
// download or native UI is needed to reproduce a response completing before poll.
async function setup(phase, statusMessage) {
  const elements = new Map();
  class Element {
    value = ''; textContent = ''; disabled = false; listeners = new Map();
    add() {}
    addEventListener(type, handler) { this.listeners.set(type, handler); }
    dispatch(type) { return this.listeners.get(type)?.(); }
  }
  for (const id of ['model-preset', 'managed-speech', 'managed-correction', 'download-models',
    'verify-models', 'apply-models', 'cancel-models', 'managed-info', 'preset-info', 'managed-status', 'model-progress']) {
    elements.set(id, new Element());
  }
  const state = { generation: 1, phase, modelId: 'base', received: 0, total: 148000000,
    index: 1, count: 1, message: statusMessage, prepared: phase === 'ready' ? [{ id: 'base', path: '/models/base.bin' }] : [] };
  let poll;
  let preparationCalls = 0;
  const context = vm.createContext({
    document: { querySelector: id => elements.get(id.slice(1)) }, Option: class {},
    window: { setInterval(handler) { poll = handler; } },
  });
  const api = new vm.SyntheticModule(['invoke', 'isTauri'], function () {
    this.setExport('isTauri', () => true);
    this.setExport('invoke', async command => {
      if (command === 'model_catalog') return [{ id: 'base', name: 'Whisper base', kind: 'transcription',
        bytes: 148000000, license: 'MIT', memoryHint: '', cached: false }];
      if (command === 'prepare_models') preparationCalls++;
      return structuredClone(state);
    });
  }, { context });
  const source = stripTypeScriptTypes(readFileSync(new URL('../../src/model-setup.ts', import.meta.url), 'utf8'));
  const module = new vm.SourceTextModule(source, { context });
  await module.link(() => api);
  await module.evaluate();
  module.namespace.setupModels(() => false, () => {});
  const settle = () => new Promise(setImmediate);
  await settle();
  elements.get('model-preset').value = 'light';
  elements.get('model-preset').dispatch('change');
  elements.get('download-models').dispatch('click');
  await settle();
  return { elements, state, settle, poll: () => poll(), calls: () => preparationCalls };
}

test('an immediate failure is visible even when the first poll returns identical state', async () => {
  const app = await setup('error', '取得処理に失敗しました。');
  assert.equal(app.elements.get('managed-status').textContent, '取得処理に失敗しました。');
  app.poll(); await app.settle();
  assert.equal(app.elements.get('managed-status').textContent, '取得処理に失敗しました。');
  assert.equal(app.elements.get('download-models').disabled, false);
  assert.equal(app.elements.get('apply-models').disabled, true);
  assert.equal(app.calls(), 1);
});

test('a cached model finishing before the first poll shows completion and enables apply', async () => {
  const app = await setup('ready', '検証完了。設定欄へ反映できます。');
  assert.equal(app.elements.get('managed-status').textContent, '検証完了。設定欄へ反映できます。');
  assert.equal(app.elements.get('apply-models').disabled, false);
  assert.equal(app.calls(), 1);
});

test('a message change with unchanged byte count is still displayed', async () => {
  const app = await setup('downloading', '取得中');
  app.state.message = '接続を確認中';
  app.poll(); await app.settle();
  assert.match(app.elements.get('managed-status').textContent, /^接続を確認中/);
  assert.equal(app.elements.get('download-models').disabled, true);
  assert.equal(app.calls(), 1);
});

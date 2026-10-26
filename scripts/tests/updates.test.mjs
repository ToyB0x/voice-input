import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';

async function setup() {
  const elements = new Map(['update-info', 'update-error', 'update-check', 'update-install'].map(id => [id, {
    textContent: '', disabled: true, listeners: new Map(), addEventListener(k, fn) { this.listeners.set(k, fn); }
  }]));
  const status = { configured: true, working: false, installing: false, currentVersion: '0.0.1', availableVersion: '0.0.2', message: '更新があります。', downloaded: 0, total: null };
  const requests = []; let interval, action;
  const context = vm.createContext({ document: { getElementById: id => elements.get(id) }, window: { setInterval(fn) { interval = fn; } } });
  const api = new vm.SyntheticModule(['invoke', 'isTauri'], function() {
    this.setExport('isTauri', () => true);
    this.setExport('invoke', async command => { requests.push(command); return command === 'update_status' ? structuredClone(status) : action(command); });
  }, { context });
  const module = new vm.SourceTextModule(stripTypeScriptTypes(readFileSync(new URL('../../src/updates.ts', import.meta.url), 'utf8')), { context });
  await module.link(() => api); await module.evaluate(); module.namespace.setupUpdates();
  const settle = () => new Promise(setImmediate); await settle();
  return { elements, status, requests, settle, module, setAction(fn) { action = fn; }, poll: async () => { interval(); await settle(); }, click: id => elements.get(id).listeners.get('click')() };
}

test('update polling preserves unchanged status nodes and button labels', async () => {
  const a = await setup(); let writes = 0;
  for (const id of ['update-info', 'update-check', 'update-install']) {
    const node = a.elements.get(id); let value = node.textContent;
    Object.defineProperty(node, 'textContent', { get() { return value; }, set(v) { value = v; writes++; } });
  }
  await a.poll(); await a.poll(); assert.equal(writes, 0);
});

test('install immediately acknowledges click and blocks duplicate requests until it settles', async () => {
  const a = await setup(); let finish;
  a.setAction(() => new Promise(resolve => { finish = resolve; }));
  a.click('update-install');
  assert.equal(a.elements.get('update-install').disabled, true);
  assert.match(a.elements.get('update-error').textContent, /確認中/);
  a.click('update-install'); await a.poll();
  assert.equal(a.requests.filter(x => x === 'install_update').length, 1);
  Object.assign(a.status, { working: true, installing: true });
  finish(structuredClone(a.status)); await a.settle();
  assert.equal(a.module.namespace.updateInstalling(), true);
});

test('blocked update shows the backend reason beside the update controls and allows retry', async () => {
  const a = await setup();
  a.setAction(async () => { throw '録音・比較結果を先に破棄してください。'; });
  a.click('update-install'); await a.settle();
  assert.match(a.elements.get('update-error').textContent, /破棄/);
  assert.equal(a.elements.get('update-install').disabled, false);
  const html = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
  assert.match(html, /id="updates-title"[\s\S]*id="update-install"[\s\S]*id="update-error"[\s\S]*<\/section>/);
});

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';

async function setup() {
  const elements = new Map();
  class Element {
    options = []; listeners = new Map(); textContent = ''; checked = false; disabled = false; open = false; inputValue = '';
    get selectedOptions() { return this.options.filter(o => o.selected); }
    get value() { return this.options.length ? this.selectedOptions[0]?.value ?? '' : this.inputValue; }
    set value(value) { this.inputValue = value; for (const o of this.options) o.selected = o.value === value; }
    add(option) { this.options.push(option); }
    addEventListener(type, callback) { this.listeners.set(type, callback); }
    dispatch(type) { this.listeners.get(type)?.(); }
    replaceChildren() { this.options = []; }
    querySelectorAll() { return [...elements.values()]; }
  }
  const html = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
  for (const [, id] of html.matchAll(/id="(compare-[^"]+|comparison-panel)"/g)) elements.set(id, new Element());
  for (const [, id, content] of html.matchAll(/<select id="(compare-[^"]+)"[^>]*>(.*?)<\/select>/gs)) {
    for (const [, value, attributes] of content.matchAll(/<option value="([^"]+)"([^>]*)>/g)) {
      elements.get(id).options.push({ value, selected: attributes.includes('selected') });
    }
    if (id === 'compare-preset-source') elements.get(id).options[0].selected = true;
  }
  let poll;
  const requests = [];
  const state = { generation: 1, working: false, armed: false, phase: 'done', total: 6, completed: 6, audioMs: 1000,
    axCharacters: 0, ocrCharacters: 12, hasReference: false, rows: [], next: 0, more: false, message: '', foundation: null };
  const context = vm.createContext({
    document: { getElementById: id => elements.get(id) },
    Option: class { selected = false; constructor(text, value) { this.text = text; this.value = value; } },
    window: { setInterval(callback) { poll = callback; } },
  });
  const api = new vm.SyntheticModule(['invoke', 'isTauri'], function () {
    this.setExport('isTauri', () => true);
    this.setExport('invoke', async (command, args) => {
      requests.push({ command, args: args && structuredClone(args) });
      if (command === 'comparison_status') return structuredClone(state);
      if (command === 'comparison_plan') return 6;
      if (command === 'comparison_models') return [
        { id: 'current-speech', name: 'Speech', kind: 'transcription', available: true },
        ...['none', 'apple-foundation', 'current-correction'].map(id => ({ id, name: id, kind: 'correction', available: true })),
      ];
    });
  }, { context });
  const source = stripTypeScriptTypes(readFileSync(new URL('../../src/comparison.ts', import.meta.url), 'utf8'));
  const module = new vm.SourceTextModule(source, { context });
  await module.link(() => api); await module.evaluate(); module.namespace.setupComparison();
  const settle = () => new Promise(setImmediate);
  await settle();
  return { elements, requests, state, settle, poll: () => poll(), click: id => elements.get(id).dispatch('click') };
}

test('Apple preset selects all context routes without requesting permissions, capturing or changing backend', async () => {
  const app = await setup();
  app.click('compare-preset-apple'); await app.settle();
  const selection = app.requests.filter(r => r.command === 'comparison_plan').at(-1).args.selection;
  assert.deepEqual(selection.speech, ['current-speech']);
  assert.deepEqual(selection.corrections, ['none', 'apple-foundation']);
  assert.deepEqual(selection.sources, ['none', 'ocr']);
  assert.deepEqual(selection.routes, ['none', 'asr-only', 'correction-only', 'both']);
  assert.deepEqual(selection.speechBackends, ['metal']);
  assert.equal(selection.reference, null);
  assert.ok(app.requests.every(r => ['comparison_plan', 'comparison_status', 'comparison_models'].includes(r.command)));
  assert.equal(app.elements.get('compare-arm').checked, false);
});

test('Qwen preset respects explicit source and reference, and report text remains opt in', async () => {
  const app = await setup();
  app.elements.get('compare-preset-source').value = 'ax';
  app.elements.get('compare-reference').value = 'PRIVATE_REFERENCE';
  app.click('compare-preset-qwen'); await app.settle();
  const selection = app.requests.filter(r => r.command === 'comparison_plan').at(-1).args.selection;
  assert.deepEqual(selection.corrections, ['none', 'current-correction']);
  assert.deepEqual(selection.sources, ['none', 'ax']);
  assert.equal(selection.reference, null);
  app.click('compare-copy-report'); await app.settle();
  assert.equal(app.requests.filter(r => r.command === 'copy_comparison_report').at(-1).args.includeText, false);
  app.elements.get('compare-include-text').checked = true;
  app.click('compare-copy-report'); await app.settle();
  assert.equal(app.requests.filter(r => r.command === 'copy_comparison_report').at(-1).args.includeText, true);
});

test('availability diagnostics update and clearing removes the previous comparison result', async () => {
  const app = await setup();
  app.state.foundation = { status: { availability: 'modelNotReady', os_major: 27, os_minor: 0, os_patch: 1 }, error: null, elapsedMs: 32 };
  app.poll(); await app.settle();
  assert.match(app.elements.get('compare-foundation').textContent, /modelNotReady/);
  assert.match(app.elements.get('compare-foundation').textContent, /27\.0\.1/);
  app.state.working = true;
  const before = app.requests.length;
  app.poll(); await app.settle(); app.click('compare-preset-apple'); await app.settle();
  assert.equal(app.requests.slice(before).filter(r => r.command === 'comparison_plan').length, 0);
  app.state.foundation = null; app.state.working = false;
  app.poll(); await app.settle();
  assert.doesNotMatch(app.elements.get('compare-foundation').textContent, /modelNotReady/);
});

test('fixed diagnosis needs no recording, reference, capture or permission command', async () => {
  const app = await setup();
  app.elements.get('compare-use-reference').checked = true;
  app.elements.get('compare-reference').value = 'PRIVATE_REFERENCE';
  const before = app.requests.length;
  app.click('compare-selftest-apple'); await app.settle();
  app.click('compare-selftest-both'); await app.settle();
  const requests = app.requests.slice(before);
  assert.deepEqual(requests.filter(r => r.command === 'start_correction_selftest').map(r => r.args),
    [{ includeQwen: false }, { includeQwen: true }]);
  assert.ok(requests.every(r => ['start_correction_selftest', 'comparison_status'].includes(r.command)));
  app.state.mode = 'fixed-correction'; app.state.working = true;
  app.poll(); await app.settle();
  assert.equal(app.elements.get('compare-selftest-apple').disabled, true);
  assert.equal(app.elements.get('compare-selftest-both').disabled, true);
  assert.match(app.elements.get('compare-progress').textContent, /音声・画面情報は使用しません/);
  app.click('compare-cancel'); await app.settle();
  assert.equal(app.requests.filter(r => r.command === 'cancel_comparison').at(-1).args.clear, false);
});

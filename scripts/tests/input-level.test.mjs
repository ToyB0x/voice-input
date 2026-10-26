import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';
import vm from 'node:vm';

async function setup() {
  const meter = { value: 0, setAttribute() {}, removeAttribute() {} };
  const readout = { textContent: '' };
  const status = { textContent: '' };
  let clock = 0;
  const requests = [];
  let tick;
  let interval;
  let cleared = false;
  let readable = true;
  const context = vm.createContext({ performance: { now: () => clock }, window: {
    setInterval(callback, ms) { tick = callback; interval = ms; cleared = false; return 1; },
    clearInterval() { cleared = true; },
  } });
  const api = new vm.SyntheticModule(['invoke'], function () {
    this.setExport('invoke', command => new Promise((resolve, reject) => { requests.push({ command, resolve, reject }); }));
  }, { context });
  const source = stripTypeScriptTypes(readFileSync(new URL('../../src/input-level.ts', import.meta.url), 'utf8'));
  const module = new vm.SourceTextModule(source, { context });
  await module.link(() => api); await module.evaluate();
  const controls = module.namespace.setupInputLevel(meter, readout, status, () => readable);
  return { meter, readout, status, requests, controls, advance: ms => { clock += ms; }, tick: () => tick(), interval: () => interval,
    cleared: () => cleared, setReadable: value => { readable = value; }, settle: () => new Promise(setImmediate) };
}

test('only the lightweight command is polled at 50ms while recording', async () => {
  const app = await setup();
  assert.equal(app.requests.length, 0);
  app.controls.setRecording(true);
  assert.equal(app.interval(), 50);
  assert.equal(app.requests[0].command, 'recording_level');
  app.requests[0].resolve({ phase: 'recording', level: 0.01 }); await app.settle();
  assert.ok(Math.abs(app.meter.value - 1 / 3) < 0.0001);
  assert.equal(app.readout.textContent, '-40.0 dBFS');
  app.tick();
  assert.equal(app.requests.length, 2);
});

test('slow responses never accumulate overlapping level requests', async () => {
  const app = await setup();
  app.controls.setRecording(true);
  for (let i = 0; i < 20; i++) app.tick();
  assert.equal(app.requests.length, 1);
  app.requests[0].resolve({ phase: 'recording', level: 0.2 }); await app.settle();
  app.tick(); assert.equal(app.requests.length, 2);
});

test('stopping clears the meter and ignores late responses from the previous recording', async () => {
  const app = await setup();
  app.controls.setRecording(true);
  app.controls.setRecording(false);
  assert.equal(app.cleared(), true);
  app.controls.setRecording(true);
  app.requests[0].resolve({ phase: 'recording', level: 0.8 }); await app.settle();
  assert.equal(app.meter.value, 0);
  app.tick();
  app.requests[1].resolve({ phase: 'recording', level: 0.3 }); await app.settle();
  assert.ok(app.meter.value > 0);
  assert.equal(app.readout.textContent, '-10.5 dBFS');
});

test('automatic stop resets the meter without waiting for metadata refresh', async () => {
  const app = await setup();
  app.controls.setRecording(true);
  app.requests[0].resolve({ phase: 'recorded', level: 0.9 }); await app.settle();
  assert.equal(app.meter.value, 0);
  assert.equal(app.cleared(), true);
  app.tick(); assert.equal(app.requests.length, 1);
});

test('busy actions do not poll or apply stale display updates', async () => {
  const app = await setup();
  app.setReadable(false); app.controls.setRecording(true);
  assert.equal(app.requests.length, 0);
  app.setReadable(true); app.tick();
  app.setReadable(false);
  app.requests[0].resolve({ phase: 'recording', level: 0.7 }); await app.settle();
  assert.equal(app.meter.value, 0);
});


test('a denied fast command keeps the regular snapshot visible and exposes the error', async () => {
  const app = await setup();
  app.controls.setRecording(true, 0.01);
  app.requests[0].reject('recording_level denied'); await app.settle();
  assert.ok(Math.abs(app.meter.value - 1 / 3) < 0.0001);
  assert.match(app.status.textContent, /recording_level denied/);
  app.controls.setRecording(true, 0.1);
  assert.equal(app.readout.textContent, '-20.0 dBFS');
});

test('regular updates keep working if a fast request never returns', async () => {
  const app = await setup();
  app.controls.setRecording(true, 0.01);
  app.controls.setRecording(true, 0.1);
  assert.equal(app.readout.textContent, '-20.0 dBFS');
  assert.match(app.status.textContent, /通常更新/);
  assert.equal(app.requests.length, 1);
});

test('a fresh fast update is not overwritten by an older regular snapshot', async () => {
  const app = await setup();
  app.controls.setRecording(true, 0.01);
  app.requests[0].resolve({ phase: 'recording', level: 0.1 }); await app.settle();
  app.controls.setRecording(true, 0.01);
  assert.equal(app.readout.textContent, '-20.0 dBFS');
  app.advance(300); app.controls.setRecording(true, 0.001);
  assert.equal(app.readout.textContent, '-60.0 dBFS');
});

test('zero and invalid levels are distinct from transport failures and stop resets the display', async () => {
  const app = await setup();
  app.controls.setRecording(true, NaN);
  assert.equal(app.readout.textContent, '-∞ dBFS');
  assert.equal(app.meter.value, 0);
  app.controls.setRecording(false);
  assert.equal(app.readout.textContent, '停止中');
  assert.equal(app.status.textContent, '');
});

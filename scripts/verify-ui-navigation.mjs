// Real Chromium DOM, keyboard and rendering checks using built-in CDP; no npm test dependency.
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, writeFile, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { resolve, extname, join, sep } from 'node:path';
import { tmpdir } from 'node:os';
import assert from 'node:assert/strict';

const output = resolve(process.argv[2] ?? '/tmp/voice-ui-navigation');
await mkdir(output, { recursive: true });
const root = resolve('dist');
const server = createServer(async (req, res) => {
  try {
    const path = resolve(root, '.' + new URL(req.url, 'http://localhost').pathname.replace(/\/$/, '/index.html'));
    if (!path.startsWith(root + sep)) { res.writeHead(403).end(); return; }
    res.setHeader('Content-Type', { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' }[extname(path)] ?? 'application/octet-stream');
    res.end(await readFile(path));
  } catch { res.writeHead(404).end(); }
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const profile = await mkdtemp(join(tmpdir(), 'voice-navigation-'));
const chrome = spawn(process.env.CHROME_BIN ?? '/usr/bin/chromium', ['--headless', '--no-sandbox', '--disable-dev-shm-usage', '--remote-debugging-port=0', '--user-data-dir=' + profile, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
let socket;
try {
  const endpoint = await new Promise((done, reject) => {
    const timer = setTimeout(() => reject(Error('Chromium startup deadline exceeded')), 45000);
    let log = '';
    chrome.stderr.on('data', bytes => { log += bytes; const match = log.match(/DevTools listening on (ws:\/\/[^\s]+)/); if (match) { clearTimeout(timer); done(match[1]); } });
    chrome.once('error', reject);
    chrome.once('exit', code => { clearTimeout(timer); reject(Error('Chromium exited: ' + code)); });
  });
  const port = new URL(endpoint).port;
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
  await new Promise((r, reject) => { socket.addEventListener('open', r, { once: true }); socket.addEventListener('error', reject, { once: true }); });
  let serial = 0;
  const pending = new Map();
  socket.addEventListener('message', e => { const response = JSON.parse(e.data); const p = pending.get(response.id); if (p) { pending.delete(response.id); clearTimeout(p.timer); response.error ? p.reject(Error(response.error.message)) : p.resolve(response.result); } });
  const call = (method, params = {}) => new Promise((resolve, reject) => { const id = ++serial; const timer = setTimeout(() => { pending.delete(id); reject(Error('CDP deadline: ' + method)); }, 45000); pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params })); });
  const evaluate = async expression => {
    const result = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    return result.result.value;
  };
  await call('Page.enable');
  await call('Page.addScriptToEvaluateOnNewDocument', { source: await readFile('scripts/fixtures/navigation-native.js', 'utf8') });
  await call('Emulation.setDeviceMetricsOverride', { width: 760, height: 940, deviceScaleFactor: 1, mobile: false });
  await call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` });
  await evaluate(`new Promise((done, reject) => { const until = Date.now()+10000; const timer = setInterval(() => { if (!document.querySelector('#save-preferences')?.disabled && document.querySelector('#shortcut').value) { clearInterval(timer); done(); } else if (Date.now()>until) { clearInterval(timer); reject(Error('Frontend did not initialize')); } },50); })`);
  const result = await evaluate(`(async () => {
    const check = (value, message) => { if (!value) throw Error(message); };
    const el = id => document.getElementById(id);
    const pause = () => new Promise(r => setTimeout(r, 200));
    const select = id => el('tab-'+id).click();
    const active = id => !el('panel-'+id).hidden && el('tab-'+id).getAttribute('aria-selected') === 'true';
    check(active('input'), 'Input must be the initial view');
    check(document.body.dataset.mode==='simple', 'Default mode must be simple');
    check(getComputedStyle(document.querySelector('.view-navigation')).display==='none', 'Debug navigation must start hidden');
    check(getComputedStyle(el('raw-result')).display==='none','Raw transcript must be debug-only');
    el('mode-toggle').click();
    check(document.body.dataset.mode==='debug' && el('mode-toggle').getAttribute('aria-expanded')==='true','Gear must open debug mode');
    check(document.querySelector('.save-bar').hidden, 'Clean input view should not show setup controls');
    check(document.querySelectorAll('[role=tabpanel]:not([hidden])').length===1, 'Exactly one visible panel');
    const ids = [...document.querySelectorAll('[id]')].map(n=>n.id);
    check(new Set(ids).size===ids.length,'Duplicate DOM IDs');
    const pathNode = el('model-path');
    select('models'); pathNode.value='/synthetic/unsaved.bin'; pathNode.dispatchEvent(new Event('input',{bubbles:true}));
    select('settings'); el('auto-correction').checked=true; el('auto-correction').dispatchEvent(new Event('input',{bubbles:true}));
    select('diagnostics'); el('comparison-panel').open=true; el('compare-reference').value='Synthetic reference';
    select('input'); await pause();
    check(!document.querySelector('.save-bar').hidden, 'Unsaved settings lost their save control');
    el('mode-toggle').click();
    check(document.body.dataset.mode==='simple' && active('input'), 'Close must restore home');
    check(el('mode-toggle').dataset.dirty==='true','Hidden unsaved settings must remain discoverable');
    check(document.querySelector('.save-bar').hidden,'Simple mode must hide debug save bar');
    el('mode-toggle').click();
    select('models');
    check(el('model-path')===pathNode && pathNode.value==='/synthetic/unsaved.bin','Navigation lost unsaved model input');
    check(!window.__ui.saved,'Navigation saved settings');
    el('save-preferences').click(); await pause();
    check(window.__ui.saved?.modelPath==='/synthetic/unsaved.bin' && window.__ui.saved.autoCorrection===true,'Shared save lost fields across panels');
    select('diagnostics'); check(el('compare-reference').value==='Synthetic reference','Navigation cleared comparison input');
    select('input'); check(document.querySelector('.save-bar').hidden, 'Saved settings still obscure routine input');
    window.__ui.snapshot.phase='recording'; window.__ui.snapshot.elapsedMs=1234; await pause();
    select('settings'); await pause(); el('mode-toggle').click(); await pause(); el('mode-toggle').click(); select('input');
    check(!el('stop').disabled && el('duration').textContent.includes('1.2'),'Navigation interrupted recording UI');
    const reads = new Set(['demo_status','context_lab_status','recording_status','recording_level','desktop_status','insertion_status','context_status','correction_status','comparison_status','comparison_plan','comparison_models','model_catalog','model_preparation_status','update_status']);
    const forbidden = window.__ui.calls.filter(c => !reads.has(c) && c !== 'save_preferences');
    check(window.__ui.calls.filter(c=>c==='save_preferences').length===1,'Settings were saved outside the explicit action');
    check(forbidden.length===0,'Navigation triggered native side effects: '+forbidden.join(','));
    el('tab-input').focus(); el('tab-input').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));
    check(active('demo') && document.activeElement===el('tab-demo'),'ArrowRight did not select and focus next tab');
    el('tab-demo').dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true})); check(active('diagnostics'),'End did not select last tab');
    el('tab-diagnostics').dispatchEvent(new KeyboardEvent('keydown',{key:'Home',bubbles:true})); check(active('input'),'Home did not return to first tab');
    el('tab-input').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true})); check(active('diagnostics'),'ArrowLeft did not wrap');
    window.__ui.snapshot.phase='idle';window.__ui.snapshot.working=false;await pause();
    select('models');await pause();
    el('usage-prepare').click();await pause();
    check(!el('usage-apply').disabled,'Verified preset cannot be applied');
    el('usage-apply').click();
    for(let i=0;i<20&&!el('usage-status').textContent.includes('準備と設定が完了');i++)await pause();
    check(window.__ui.preferences.usagePreset==='asr-first' && window.__ui.preferences.autoCorrection===false,'Preset did not save the intended mode');
    check(el('model-path').value==='/synthetic/large-v3-turbo','Preset did not update the actual model setting');
    check(el('usage-status').textContent.includes('準備と設定が完了'),'Preset setup did not reach ready');
    check(window.__ui.calls.filter(c=>c==='apply_usage_preset').length===1 && window.__ui.calls.filter(c=>c==='load_model').length===1,'Preset setup did not explicitly save and load once');
    select('input');
    return {presetSetup:true,status:'passed' , realBrowser:true, nativeBackend:'synthetic', panels:5, keyboardNavigation:true, fieldsAndRecordingPreserved:true, explicitSharedSave:true, navigationSideEffects:false};
  })()`);
  assert.equal(result.status, 'passed');
  await evaluate(`document.getElementById('tab-input').click(); window.scrollTo(0,0);`);
  const tabPoint = await evaluate(`(() => { const r=document.getElementById('tab-models').getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()`);
  for (const type of ['mousePressed','mouseReleased']) await call('Input.dispatchMouseEvent', {type,...tabPoint,button:'left',clickCount:1});
  assert.equal(await evaluate(`document.getElementById('tab-models').getAttribute('aria-selected')`),'true');
  await evaluate(`document.getElementById('tab-input').focus();`);
  for (const type of ['keyDown','keyUp']) await call('Input.dispatchKeyEvent',{type,key:'ArrowRight',code:'ArrowRight',windowsVirtualKeyCode:39});
  assert.equal(await evaluate(`document.getElementById('tab-demo').getAttribute('aria-selected')`),'true');
  result.pointerAndKeyboardEvents = true;
  result.demo = await evaluate(`(async () => {
    const el=id=>document.getElementById(id), pause=()=>new Promise(r=>setTimeout(r,1700));
    el('tab-demo').click();
    if (!el('demo-run-image').disabled) throw Error('Missing demo models did not disable execution');
    window.__ui.demoReady=true; await pause();
    const preferences=JSON.stringify(window.__ui.preferences);
    if (el('demo-gpu').checked) throw Error('GPU must be opt-in');
    for (const stage of ['audio','text','image']) {
      el('demo-gpu').checked = stage !== 'audio';
      el('demo-run-'+stage).click();await pause();
      const options=window.__ui.demoOptions;
      if (options?.stage!==stage || options.useGpu !== (stage !== 'audio') || options.repeats!==1 || options.texts[0]!=='今日は台風だ。') throw Error('Wrong demo preset');
      if (!el('demo-gpu').disabled || !el('demo-run-audio').disabled || el('demo-cancel').disabled) throw Error('Busy demo controls are unsafe');
      el('demo-cancel').click();await pause();
    }
    el('demo-resident').checked=true;el('demo-resident').dispatchEvent(new Event('change'));
    if(!el('demo-run-audio').disabled || el('demo-warmup').disabled) throw Error('Resident preparation gate failed');
    el('demo-warmup').click();await pause();
    if(window.__ui.demoOptions.stage!=='prepare' || !window.__ui.demoOptions.resident) throw Error('Wrong resident preparation request');
    el('demo-cancel').click();await pause();
    window.__ui.demo={...window.__ui.demo,residentReadyMetal:true};await pause();
    if(el('demo-run-audio').disabled) throw Error('Prepared resident cannot run');
    el('demo-run-audio').click();await pause();
    if(!window.__ui.demoOptions.resident || !window.__ui.demoOptions.useGpu) throw Error('Resident/backend selection lost');
    el('demo-cancel').click();await pause();
    el('demo-gpu').checked=false;el('demo-gpu').dispatchEvent(new Event('change'));
    if(!el('demo-run-audio').disabled) throw Error('Wrong backend reused resident preparation');
    el('demo-resident').checked=false;el('demo-resident').dispatchEvent(new Event('change'));
    if(JSON.stringify(window.__ui.preferences)!==preferences) throw Error('Demo changed ordinary preferences');
    const audio=el('demo-audio');audio.load();await new Promise((resolve,reject)=>{audio.onloadedmetadata=resolve;audio.onerror=()=>reject(Error('Bundled demo audio failed'));});
    if(!(audio.duration>1 && audio.duration<3)) throw Error('Wrong demo audio');
    return {presets:true,missingModels:true,busyCancellation:true,settingsPreserved:true,audioDuration:audio.duration,nativeBackend:'synthetic'};
  })()`);
  await evaluate(`window.__ui.snapshot.phase='idle'; new Promise(r=>setTimeout(r,200));`);
  await evaluate(`document.getElementById('mode-toggle').click(); window.__ui.snapshot={...window.__ui.snapshot,phase:'recorded',sampleCount:16000,transcript:'雨がすごいですね。',finalText:'雨がすごいですね。',finalSource:'whisper',elapsedMs:2100}; new Promise(r=>setTimeout(r,1800));`);
  await evaluate(`(async()=>{
    window.__ui.insertion={enabled:true,trusted:true,state:'blocked',message:'入力先を確認できません。'};
    await new Promise(r=>setTimeout(r,1800));
    if(document.getElementById('home-notice').hidden || !document.getElementById('home-notice').textContent.includes('入力先')) throw Error('Home hid insertion failure');
    window.__ui.insertion=undefined;
    await new Promise(r=>setTimeout(r,1800));
  })()`);
  for (const [name,width,height,color] of [['home-light',960,820,'light'],['home-dark',960,820,'dark'],['home-narrow',360,950,'light']]) {
    await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:2,mobile:false});
    await call('Emulation.setEmulatedMedia',{features:[{name:'prefers-color-scheme',value:color}]});
    await evaluate(`window.scrollTo(0,0); document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))`);
    assert.equal(await evaluate('document.documentElement.scrollWidth <= innerWidth'),true,'Home overflow: '+name);
    const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});
    await writeFile(join(output,name+'.png'),Buffer.from(shot.data,'base64'));
  }
  result.simpleHome={default:true,gearOnlyDebug:true,unsavedPreserved:true,recordingPreserved:true,screenshots:'synthetic result in actual frontend'};
  await evaluate(`document.getElementById('mode-toggle').click();`);
  for (const [name, width, height, color, view] of [['input-light',760,940,'light','input'],['models-light',760,940,'light','models'],['settings-dark',760,940,'dark','settings'],['diagnostics-light',760,940,'light','diagnostics'],['input-narrow',360,620,'light','input'],['demo-light',760,940,'light','demo'],['demo-narrow',360,620,'light','demo']]) {
    await call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
    await call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: color }] });
    await evaluate(`document.getElementById('tab-${view}').click(); document.getElementById('tab-${view}').focus({preventScroll:true}); window.scrollTo(0,0);`);
    const layout = await evaluate(`({width:document.documentElement.scrollWidth, viewport:innerWidth, active:document.querySelectorAll('[role=tabpanel]:not([hidden])').length})`);
    assert.equal(layout.active, 1); assert.ok(layout.width <= width, 'Horizontal overflow at ' + name);
    await evaluate(`document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))`);
    const capture = await call('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    await writeFile(join(output, name+'.png'), Buffer.from(capture.data,'base64'));
  }
  await writeFile(join(output, 'result.json'), JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify(result));
} finally {
  socket?.close(); chrome.kill(); server.closeAllConnections(); server.close();
  await new Promise(r => chrome.exitCode!==null ? r() : chrome.once('exit',r));
  // Chromium utility processes may finish profile writes just after its parent exits.
  await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
}

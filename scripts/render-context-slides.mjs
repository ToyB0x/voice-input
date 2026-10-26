// Built-in CDP only: check actual font-ready layout, save every slide and print the PDF.
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, writeFile, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { tmpdir } from 'node:os';
import assert from 'node:assert/strict';

const output = resolve(process.argv[2]);
await mkdir(output, { recursive: true });
const html = await readFile(join(output, 'slides.html'));
const server = createServer((req, res) => {
  if (req.url !== '/slides') { res.writeHead(404).end(); return; }
  res.setHeader('Content-Type', 'text/html; charset=utf-8'); res.end(html);
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const profile = await mkdtemp(join(tmpdir(), 'voice-context-slides-'));
const chrome = spawn(process.env.CHROME_BIN ?? '/usr/bin/chromium', ['--headless', '--no-sandbox', '--disable-dev-shm-usage', '--remote-debugging-port=0', '--user-data-dir=' + profile, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
let socket;
try {
  const endpoint = await new Promise((done, reject) => {
    const timer = setTimeout(() => reject(Error('Chromium startup deadline')), 15000);
    let log = '';
    chrome.stderr.on('data', bytes => { log += bytes; const match = log.match(/DevTools listening on (ws:\/\/[^\s]+)/); if (match) { clearTimeout(timer); done(match[1]); } });
    chrome.once('error', reject); chrome.once('exit', code => { clearTimeout(timer); reject(Error('Chromium exited: ' + code)); });
  });
  const targets = await (await fetch(`http://127.0.0.1:${new URL(endpoint).port}/json/list`)).json();
  socket = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
  await new Promise((r, reject) => { socket.addEventListener('open', r, { once: true }); socket.addEventListener('error', reject, { once: true }); });
  let serial = 0; const pending = new Map();
  socket.addEventListener('message', e => { const r = JSON.parse(e.data), p = pending.get(r.id); if (p) { pending.delete(r.id); clearTimeout(p.timer); r.error ? p.reject(Error(r.error.message)) : p.resolve(r.result); } });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++serial, timer = setTimeout(() => { pending.delete(id); reject(Error('CDP deadline: ' + method)); }, 20000);
    pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async expression => {
    const r = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  };
  await call('Page.enable');
  await call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 720, deviceScaleFactor: 1, mobile: false });
  await call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/slides` });
  await evaluate(`new Promise((done,reject)=>{const deadline=Date.now()+10000;const timer=setInterval(()=>{if(document.querySelector('section')){clearInterval(timer);done()}else if(Date.now()>deadline){clearInterval(timer);reject(Error('No slides loaded'))}},25)})`);
  await evaluate(`document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))`);
  const layout = await evaluate(`(() => {
    const sections=[...document.querySelectorAll('section')];
    return {realBrowser:true,source:'actual measured report',slides:sections.map((s,i)=>{
      const box=s.getBoundingClientRect(),limit=s.querySelector('footer').getBoundingClientRect().top-8;
      const clipped=[...s.children].filter(c=>c.tagName!=='FOOTER' && c.getBoundingClientRect().bottom>limit).map(c=>c.tagName);
      return {page:i+1,title:s.querySelector('h1').textContent,width:box.width,height:box.height,clipped};
    })};
  })()`);
  layout.status = layout.slides.length && layout.slides.every(s => !s.clipped.length && s.width === 1280 && s.height === 720) ? 'passed' : 'failed';
  await writeFile(join(output, 'layout.json'), JSON.stringify(layout, null, 2) + '\n');
  for (let i = 0; i < layout.slides.length; i++) {
    const clip = await evaluate(`(() => {const r=document.querySelectorAll('section')[${i}].getBoundingClientRect();return {x:r.x+scrollX,y:r.y+scrollY,width:r.width,height:r.height,scale:1}})()`);
    const png = await call('Page.captureScreenshot', { format: 'png', clip, captureBeyondViewport: true });
    await writeFile(join(output, `slide-${String(i + 1).padStart(2, '0')}.png`), Buffer.from(png.data, 'base64'));
  }
  assert.equal(layout.status, 'passed', 'Slide overflow; see layout.json and slide PNGs');
  const pdf = await call('Page.printToPDF', { printBackground: true, preferCSSPageSize: true });
  await writeFile(join(output, 'slides.pdf'), Buffer.from(pdf.data, 'base64'));
  console.log(JSON.stringify({ status: layout.status, slides: layout.slides.length, pdf: 'slides.pdf' }));
} finally {
  socket?.close(); chrome.kill(); server.closeAllConnections(); server.close();
  await new Promise(r => chrome.exitCode !== null ? r() : chrome.once('exit', r));
  await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
}

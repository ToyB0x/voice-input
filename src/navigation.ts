// Panels stay mounted: navigation never saves preferences or starts native work.
const tabs = [...document.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
const panels = [...document.querySelectorAll<HTMLElement>('[role="tabpanel"]')];
const scrollPositions = new Map<string, number>();
let current = 'input';

function select(view: string, focusTab = false) {
  const selected = tabs.find(tab => tab.id === `tab-${view}`);
  if (!selected) return;
  scrollPositions.set(current, window.scrollY);
  current = view;
  for (const tab of tabs) {
    const active = tab === selected;
    tab.setAttribute('aria-selected', String(active));
    tab.tabIndex = active ? 0 : -1;
  }
  for (const panel of panels) panel.hidden = panel.id !== `panel-${view}`;
  updateSaveBar();
  if (focusTab) selected.focus({ preventScroll: true });
  window.scrollTo({ top: scrollPositions.get(view) ?? 0, behavior: 'instant' });
}

for (const tab of tabs) {
  tab.addEventListener('click', () => select(tab.id.slice(4)));
  tab.addEventListener('keydown', event => {
    const index = tabs.indexOf(tab);
    const next = event.key === 'ArrowRight' ? (index + 1) % tabs.length
      : event.key === 'ArrowLeft' ? (index + tabs.length - 1) % tabs.length
      : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault();
    select(tabs[next].id.slice(4), true);
  });
}
for (const link of document.querySelectorAll<HTMLButtonElement>('[data-view-target]')) {
  link.addEventListener('click', () => select(link.dataset.viewTarget!, true));
}

const saveBar = document.querySelector<HTMLElement>('.save-bar')!;
const preferenceMessage = document.querySelector<HTMLElement>('#preferences-message')!;
const unsaved = document.querySelector<HTMLElement>('#unsaved-status')!;
let dirty = false;
const preferenceFields = new Set(['model-path', 'backend', 'shortcut', 'auto-insert', 'auto-correction', 'reuse-correction-model', 'show-overlay', 'recognition-context', 'ocr-context', 'whisper-context', 'correction-context', 'correction-instruction', 'correction-engine', 'correction-model-path', 'correction-backend']);
function updateSaveBar() {
  saveBar.hidden = !dirty && !['models', 'settings'].includes(current);
  unsaved.textContent = dirty ? '未保存の変更があります。' : '';
}
document.addEventListener('input', event => {
  if (event.target instanceof HTMLElement && preferenceFields.has(event.target.id)) {
    dirty = true;
    updateSaveBar();
  }
});
new MutationObserver(() => {
  const message = preferenceMessage.textContent ?? '';
  if (message.startsWith('設定を保存しました。')) dirty = false;
  else if (message.includes('設定欄へ反映') || message.includes('補正モデルの設定を反映')) dirty = true;
  updateSaveBar();
}).observe(preferenceMessage, { childList: true, characterData: true, subtree: true });
updateSaveBar();

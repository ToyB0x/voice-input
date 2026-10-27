// Synthetic frontend fixture only. No audio, screen capture or real permissions.
window.isTauri = true;
window.__ui = {
  calls: [], saved: null,
  snapshot: { phase: 'idle', permission: 'authorized', elapsedMs: 0, level: 0, sampleCount: 0, sampleRate: 16000,
    modelReady: true, modelName: 'base', modelBytes: 148000000, backend: 'cpu', working: false, recognitionPhase: 'idle',
    transcript: null, finalText: null, finalSource: '', recognitionError: null },
  preferences: { version: 1, modelPath: '/synthetic/base.bin', backend: 'cpu', shortcut: 'Command+Shift+Space',
    autoInsert: false, autoCorrection: false, reuseCorrectionModel: false, showOverlay: false,
    recognitionContext: false, ocrContext: false, whisperContext: true, correctionContext: false,
    correctionModelPath: '', correctionEngine: 'qwen3', correctionBackend: 'cpu' },
};
window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
  const s = window.__ui; s.calls.push(command);
  if (command === 'recording_status') return structuredClone(s.snapshot);
  if (command === 'recording_level') return { phase: s.snapshot.phase, level: 0.02 };
  if (command === 'save_preferences') { s.saved = structuredClone(args.preferences); s.preferences = s.saved; }
  if (['desktop_status','save_preferences'].includes(command)) return { preferences: structuredClone(s.preferences), shortcutRegistered: true, error: null };
  if (command === 'insertion_status') return { enabled: false, trusted: false };
  if (command === 'context_status') return { message: '認識補助: 無効', ocrMessage: 'OCR: 無効', screenPermitted: false };
  if (command === 'correction_status') return { working: false, checkingFoundation: false, foundation: null, text: null, error: null, elapsedMs: 0 };
  if (command === 'context_lab_status') return {runId:null,working:false,phase:'idle',message:'',planned:0,completed:0,rows:[],next:0,more:false,parts:0,foundation:null};
  if (command === 'comparison_status') return { generation: 1, rows: [], next: 0, armed: false, working: false, completed: 0, total: 0, phase: 'idle', message: '', mode: 'recorded', audioMs: 0, axCharacters: 0, ocrCharacters: 0 };
  if (command === 'comparison_plan') return 1;
  if (command === 'comparison_models') return [];
  if (command === 'prepare_models') { s.prepared={generation:1,phase:'ready',prepared:args.ids.map(id=>({id,path:'/synthetic/'+id})),message:'検証完了',total:0,received:0,index:1,count:args.ids.length}; return structuredClone(s.prepared); }
  if (command === 'apply_usage_preset') { s.preferences={...args.preferences,usagePreset:args.id,modelPath:'/synthetic/large-v3-turbo',backend:'metal',autoCorrection:false,recognitionContext:false,ocrContext:false,correctionContext:false,correctionInstruction:''};return {preferences:structuredClone(s.preferences),shortcutRegistered:true,error:null}; }
  if (command === 'load_model') { s.snapshot={...s.snapshot,phase:'idle',working:false,modelReady:true,backend:args.backend,modelName:'large-v3-turbo',modelBytes:1624555275};return structuredClone(s.snapshot); }
  if (command === 'model_catalog') return [{id:'large-v3-turbo',kind:'transcription',name:'Whisper large-v3-turbo',bytes:1624555275,cached:true},{id:'qwen3-4b',kind:'correction',name:'Qwen3 4B',bytes:2497280256,cached:false},{ id: 'base', kind: 'transcription', name: 'Whisper base', bytes: 148000000, license: 'MIT', memoryHint: '', cached: true }];
  if (command === 'model_preparation_status') return s.prepared ?? { generation: 0, phase: 'idle', prepared: [], message: '', total: 0, received: 0 };
  if (command === 'update_status') return { configured: false, working: false, installing: false, currentVersion: '0.0.1', availableVersion: null, phase: 'idle', message: '更新署名キー未設定', downloaded: 0, total: null };
  throw new Error('Unexpected native command: ' + command);
}};

#!/usr/bin/env python3
"""Setup only: synthesize one shared Japanese WAV and render public fixture windows."""
import argparse, hashlib, json, subprocess, wave
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--output', type=Path, required=True)
p.add_argument('--native', type=Path, required=True)
a=p.parse_args(); a.output.mkdir(parents=True, exist_ok=True)
# Explicitly Japanese system voice. If absent, fail instead of silently using English.
voices=subprocess.check_output(['say','-v','?'],text=True)
voice=next((line.split(' ja_JP')[0].strip() for line in voices.splitlines() if ' ja_JP ' in line),None)
if not voice: raise SystemExit('No installed Japanese TTS voice; audio experiment unavailable')
utterance='あめがすごいですね'
subprocess.run(['say','-v',voice,'-r','155','-o',str(a.output/'speech.aiff'),utterance],check=True,timeout=90)
subprocess.run(['afconvert',str(a.output/'speech.aiff'),str(a.output/'speech.wav'),'-f','WAVE','-d','LEI16@16000','-c','1'],check=True,timeout=30)
with wave.open(str(a.output/'speech.wav'),'rb') as w:
 assert w.getframerate()==16000 and w.getnchannels()==1 and w.getsampwidth()==2
 audio_ms=w.getnframes()*1000//16000
for scene in ['storm','candy']:
 subprocess.run([str(a.native.resolve()),str(ROOT/'evaluation/context-evidence/assets'),str((a.output/scene).resolve()),scene],check=True,timeout=90)
manifest=dict(schemaVersion=1,utterance=utterance,voice=voice,origin='macOS installed Japanese say voice; synthetic, not human speech',audioMs=audio_ms,
 hashes={str(f.relative_to(a.output)):hashlib.sha256(f.read_bytes()).hexdigest() for f in a.output.rglob('*') if f.is_file()},
 assetHashes={f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in (ROOT/'evaluation/context-evidence/assets').glob('*.png')})
(a.output/'fixtures.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')

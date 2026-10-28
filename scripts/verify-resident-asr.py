#!/usr/bin/env python3
"""Verify a real resident Whisper process loads once and recognizes each PCM request."""
import argparse,json,subprocess,time,wave,struct
from pathlib import Path
p=argparse.ArgumentParser()
p.add_argument('--binary',type=Path,required=True);p.add_argument('--model',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
root=Path(__file__).resolve().parents[1]
with wave.open(str(root/'evaluation/context-evidence/baseline/fixtures/speech.wav')) as f:
    assert f.getnchannels()==1 and f.getframerate()==16000 and f.getsampwidth()==2
    pcm=f.readframes(f.getnframes())
audio=[v[0]/32768 for v in struct.iter_unpack('<h',pcm)]
rows=[]
child=subprocess.Popen([str(a.binary.resolve()),str(a.model.resolve()),'cpu','--serve'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
try:
    for index in range(3):
        start=time.monotonic()
        child.stdin.write(json.dumps({'audio':audio}).encode()+b'\n');child.stdin.flush()
        row=json.loads(child.stdout.readline())
        row.update(warmup=index==0,roundtrip_ms=round((time.monotonic()-start)*1000));rows.append(row)
        assert row['text'].rstrip('。')=='雨がすごいですね',row
        assert row['model_reused']==(index>0),row
        if index>0:assert row['load_ms']==0,row
    child.stdin.close();assert child.wait(timeout=10)==0
finally:
    if child.poll() is None:child.kill();child.wait()
a.output.parent.mkdir(parents=True,exist_ok=True)
a.output.write_text(json.dumps({'complete':True,'rows':rows},ensure_ascii=False,indent=2)+'\n')
